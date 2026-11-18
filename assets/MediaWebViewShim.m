// Media-page support for the app's WKWebView on iOS and macOS.
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
