// Media-page support and blank-page recovery for the app's WKWebView on iOS
// and macOS. The recovery part is at the end of this file.
//
// The voice, camera and recording pages are station documents shown in
// iframes. When the operator has chosen to skip TLS verification for the
// station (its HTTPS certificate is self-signed), the Rust side pins the exact
// certificate it saw for that host here. WKWebView then accepts that one
// certificate for that one host, so the pages load over HTTPS, where the
// microphone and camera are available. Every other host and certificate keeps
// WebKit's default handling.
//
// wry's navigation delegate does not implement the authentication-challenge
// method, so it is added to the delegate's class at run time. WKWebView caches
// which methods its delegate implements when the delegate is assigned, so the
// delegate is assigned again afterwards.

#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import <WebKit/WebKit.h>
#import <objc/runtime.h>
#include <TargetConditionals.h>
#if TARGET_OS_IPHONE
#import <UIKit/UIKit.h>
#else
#import <AppKit/AppKit.h>
#endif

static NSMutableDictionary<NSString *, NSData *> *GS26PinnedCertificates(void) {
    static NSMutableDictionary<NSString *, NSData *> *pinned;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        pinned = [NSMutableDictionary dictionary];
    });
    return pinned;
}

static NSData *GS26PinnedCertificateForHost(NSString *host) {
    if (host.length == 0) {
        return nil;
    }
    NSMutableDictionary<NSString *, NSData *> *pinned = GS26PinnedCertificates();
    @synchronized (pinned) {
        return pinned[host.lowercaseString];
    }
}

static NSData *GS26LeafCertificate(SecTrustRef trust) {
    SecCertificateRef leaf = NULL;
    if (@available(iOS 15.0, macOS 12.0, *)) {
        CFArrayRef chain = SecTrustCopyCertificateChain(trust);
        if (chain != NULL) {
            if (CFArrayGetCount(chain) > 0) {
                leaf = (SecCertificateRef)CFRetain(CFArrayGetValueAtIndex(chain, 0));
            }
            CFRelease(chain);
        }
    } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        if (SecTrustGetCertificateCount(trust) > 0) {
            SecCertificateRef first = SecTrustGetCertificateAtIndex(trust, 0);
            if (first != NULL) {
                leaf = (SecCertificateRef)CFRetain(first);
            }
        }
#pragma clang diagnostic pop
    }
    if (leaf == NULL) {
        return nil;
    }
    NSData *der = (__bridge_transfer NSData *)SecCertificateCopyData(leaf);
    CFRelease(leaf);
    return der;
}

static void GS26DidReceiveAuthenticationChallenge(
    id self,
    SEL _cmd,
    WKWebView *webView,
    NSURLAuthenticationChallenge *challenge,
    void (^completionHandler)(NSURLSessionAuthChallengeDisposition, NSURLCredential *)) {
    (void)self;
    (void)_cmd;
    (void)webView;
    NSURLProtectionSpace *space = challenge.protectionSpace;
    if ([space.authenticationMethod isEqualToString:NSURLAuthenticationMethodServerTrust]
        && space.serverTrust != NULL) {
        NSData *pinned = GS26PinnedCertificateForHost(space.host);
        NSData *presented = pinned ? GS26LeafCertificate(space.serverTrust) : nil;
        if (pinned != nil && presented != nil && [presented isEqualToData:pinned]) {
            completionHandler(NSURLSessionAuthChallengeUseCredential,
                              [NSURLCredential credentialForTrust:space.serverTrust]);
            return;
        }
    }
    completionHandler(NSURLSessionAuthChallengePerformDefaultHandling, nil);
}

void gs26_webview_install_media_policy(void *webview_ptr) {
    if (webview_ptr == NULL) {
        return;
    }
    void (^install)(void) = ^{
        WKWebView *webView = (__bridge WKWebView *)webview_ptr;
        id<WKNavigationDelegate> delegate = webView.navigationDelegate;
        if (delegate == nil) {
            return;
        }
        SEL selector = @selector(webView:didReceiveAuthenticationChallenge:completionHandler:);
        if (![(id)delegate respondsToSelector:selector]) {
            class_addMethod(object_getClass(delegate), selector,
                            (IMP)GS26DidReceiveAuthenticationChallenge, "v@:@@@?");
        }
        // Re-assign so WebKit re-reads the delegate's implemented methods.
        // `delegate` is a strong local reference, and wry also retains it.
        webView.navigationDelegate = nil;
        webView.navigationDelegate = delegate;
    };
    if ([NSThread isMainThread]) {
        install();
    } else {
        dispatch_sync(dispatch_get_main_queue(), install);
    }
}

void gs26_webview_trust_certificate(const char *host, const uint8_t *der, size_t len) {
    if (host == NULL || der == NULL || len == 0) {
        return;
    }
    NSString *key = [[NSString stringWithUTF8String:host] lowercaseString];
    if (key.length == 0) {
        return;
    }
    NSData *certificate = [NSData dataWithBytes:der length:len];
    NSMutableDictionary<NSString *, NSData *> *pinned = GS26PinnedCertificates();
    @synchronized (pinned) {
        pinned[key] = certificate;
    }
}

// ---------------------------------------------------------------------------
// Blank page after returning to the foreground.
//
// WebKit may end the web content process while the app is in the background.
// wry's navigation delegate already implements
// webViewWebContentProcessDidTerminate: and dioxus-desktop reloads the app
// document from it (vendor-dioxus-desktop/src/recovery.rs). This check covers a
// page that died without that callback, or came back empty: each time the app
// becomes active the page is asked whether its root has content. A page that
// errors (no content process) or stays empty past a short grace period is
// reloaded through Rust, which lets the reload past the navigation policy and
// remounts the app on the same route.

void gs26_webview_request_reload(const char *reason);

static NSString *const GS26RecoveryProbe =
    @"(function(){var m=document.getElementById('main');"
     "return !!(m&&m.childElementCount>0);})()";
static const NSTimeInterval GS26RecoveryGraceSeconds = 2.0;
static const NSTimeInterval GS26RecoveryMinIntervalSeconds = 15.0;

static __weak WKWebView *GS26RecoveryWebView;
static NSDate *GS26LastRecoveryReload;
static BOOL GS26RecoveryCheckRunning;
static dispatch_once_t GS26RecoveryObserverOnce;

// Calls back with YES when the page answered and its root has content.
static void GS26ProbePage(WKWebView *webView, void (^done)(BOOL healthy, NSString *detail)) {
    if (webView.URL == nil) {
        done(NO, @"no document");
        return;
    }
    [webView evaluateJavaScript:GS26RecoveryProbe
              completionHandler:^(id result, NSError *error) {
        if (error != nil) {
            done(NO, @"page did not answer");
        } else if (![result respondsToSelector:@selector(boolValue)] || ![result boolValue]) {
            done(NO, @"page root is empty");
        } else {
            done(YES, nil);
        }
    }];
}

static void GS26RequestRecoveryReload(NSString *detail) {
    NSDate *now = [NSDate date];
    if (GS26LastRecoveryReload != nil
        && [now timeIntervalSinceDate:GS26LastRecoveryReload] < GS26RecoveryMinIntervalSeconds) {
        return;
    }
    GS26LastRecoveryReload = now;
    NSString *reason = [NSString stringWithFormat:@"blank after becoming active: %@", detail];
    NSLog(@"[webview] content process terminated, reloading (%@)", reason);
    gs26_webview_request_reload(reason.UTF8String);
}

static void GS26CheckPageAfterActivation(void) {
    WKWebView *webView = GS26RecoveryWebView;
    if (webView == nil || GS26RecoveryCheckRunning) {
        return;
    }
    GS26RecoveryCheckRunning = YES;
    GS26ProbePage(webView, ^(BOOL healthy, NSString *firstDetail) {
        if (healthy) {
            GS26RecoveryCheckRunning = NO;
            return;
        }
        // The page may still be rendering (or the edit socket reconnecting).
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW,
                                     (int64_t)(GS26RecoveryGraceSeconds * NSEC_PER_SEC)),
                       dispatch_get_main_queue(), ^{
            WKWebView *again = GS26RecoveryWebView;
            if (again == nil) {
                GS26RecoveryCheckRunning = NO;
                return;
            }
            GS26ProbePage(again, ^(BOOL healthyNow, NSString *detail) {
                GS26RecoveryCheckRunning = NO;
                if (!healthyNow) {
                    GS26RequestRecoveryReload(detail ?: firstDetail);
                }
            });
        });
    });
}

void gs26_webview_install_recovery(void *webview_ptr) {
    if (webview_ptr == NULL) {
        return;
    }
    void (^install)(void) = ^{
        GS26RecoveryWebView = (__bridge WKWebView *)webview_ptr;
        dispatch_once(&GS26RecoveryObserverOnce, ^{
#if TARGET_OS_IPHONE
            NSNotificationName name = UIApplicationDidBecomeActiveNotification;
#else
            NSNotificationName name = NSApplicationDidBecomeActiveNotification;
#endif
            [[NSNotificationCenter defaultCenter]
                addObserverForName:name
                            object:nil
                             queue:[NSOperationQueue mainQueue]
                        usingBlock:^(NSNotification *note) {
                (void)note;
                GS26CheckPageAfterActivation();
            }];
        });
    };
    if ([NSThread isMainThread]) {
        install();
    } else {
        dispatch_sync(dispatch_get_main_queue(), install);
    }
}
