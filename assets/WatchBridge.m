// Phone side of the Apple Watch companion (iOS only). Rust publishes snapshot
// JSON and polls watch messages through the C functions below; this file only
// moves strings between WCSession and a small queue. It never interprets them.
#import <Foundation/Foundation.h>
#import <WatchConnectivity/WatchConnectivity.h>

static const NSUInteger kMaxQueued = 32;
static const NSUInteger kMaxMessageBytes = 32 * 1024;

@interface GS26WatchBridge : NSObject <WCSessionDelegate>
@property(nonatomic, strong) NSMutableArray<NSString *> *incoming;
@property(nonatomic, copy) NSString *pendingContext;
@end

@implementation GS26WatchBridge

+ (instancetype)shared {
  static GS26WatchBridge *bridge;
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    bridge = [GS26WatchBridge new];
    bridge.incoming = [NSMutableArray array];
  });
  return bridge;
}

- (void)start {
  if (![WCSession isSupported]) return;
  WCSession *session = [WCSession defaultSession];
  if (session.delegate != self) {
    session.delegate = self;
  }
  // Safe to repeat; also recovers a session left inactive or not activated.
  if (session.activationState != WCSessionActivationStateActivated) {
    [session activateSession];
  }
}

- (void)enqueue:(id)value {
  if (![value isKindOfClass:[NSString class]]) return;
  NSString *text = value;
  if ([text lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > kMaxMessageBytes) return;
  @synchronized(self) {
    if (self.incoming.count >= kMaxQueued) [self.incoming removeObjectAtIndex:0];
    [self.incoming addObject:text];
  }
}

- (void)publish:(NSString *)json urgent:(BOOL)urgent {
  if (![WCSession isSupported]) return;
  WCSession *session = [WCSession defaultSession];
  if (session.activationState != WCSessionActivationStateActivated) {
    @synchronized(self) { self.pendingContext = json; }
    if (session.activationState == WCSessionActivationStateNotActivated) {
      dispatch_async(dispatch_get_main_queue(), ^{ [self start]; });
    }
    return;
  }
  if (!session.isPaired || !session.isWatchAppInstalled) {
    // Delivered from sessionWatchStateDidChange once the app is installed.
    @synchronized(self) { self.pendingContext = json; }
    return;
  }
  @synchronized(self) { self.pendingContext = nil; }
  NSError *error = nil;
  // Latest state survives the watch app being closed or out of range.
  [session updateApplicationContext:@{@"gs26" : json} error:&error];
  if (urgent && session.isReachable) {
    [session sendMessage:@{@"gs26" : json} replyHandler:nil errorHandler:nil];
  }
}

// bit0 supported, bit1 paired, bit2 watch app installed, bit3 reachable,
// bit4 session activated. isPaired/isWatchAppInstalled are only valid once
// activation has completed, so before that only bit0 is set.
- (int)state {
  if (![WCSession isSupported]) return 0;
  WCSession *session = [WCSession defaultSession];
  int bits = 1;
  if (session.activationState == WCSessionActivationStateActivated) {
    bits |= 16;
    if (session.isPaired) bits |= 2;
    if (session.isWatchAppInstalled) bits |= 4;
    if (session.isReachable) bits |= 8;
  }
  return bits;
}

#pragma mark WCSessionDelegate

- (void)session:(WCSession *)session
    activationDidCompleteWithState:(WCSessionActivationState)activationState
                             error:(NSError *)error {
  NSString *pending;
  @synchronized(self) {
    pending = self.pendingContext;
    self.pendingContext = nil;
  }
  if (error) NSLog(@"GS26 watch: WCSession activation failed: %@", error);
  NSLog(@"GS26 watch: WCSession activation state %ld, paired %d, app installed %d, reachable %d",
        (long)activationState, session.isPaired, session.isWatchAppInstalled, session.isReachable);
  if (pending && activationState == WCSessionActivationStateActivated) {
    [self publish:pending urgent:NO];
  }
}

- (void)flushPending {
  NSString *pending;
  @synchronized(self) {
    pending = self.pendingContext;
    self.pendingContext = nil;
  }
  if (pending) [self publish:pending urgent:YES];
}

// Pairing or the watch app's install state changed (e.g. the watch app was
// just installed): resend the latest snapshot.
- (void)sessionWatchStateDidChange:(WCSession *)session {
  NSLog(@"GS26 watch: watch state changed, paired %d, app installed %d",
        session.isPaired, session.isWatchAppInstalled);
  [self flushPending];
}

- (void)sessionReachabilityDidChange:(WCSession *)session {
  if (session.isReachable) [self flushPending];
}

- (void)sessionDidBecomeInactive:(WCSession *)session {
}

- (void)sessionDidDeactivate:(WCSession *)session {
  // Switching watches: reactivate for the newly paired one.
  [session activateSession];
}

- (void)session:(WCSession *)session didReceiveMessage:(NSDictionary<NSString *, id> *)message {
  [self enqueue:message[@"gs26_watch"]];
}

- (void)session:(WCSession *)session
    didReceiveMessage:(NSDictionary<NSString *, id> *)message
         replyHandler:(void (^)(NSDictionary<NSString *, id> *))replyHandler {
  [self enqueue:message[@"gs26_watch"]];
  replyHandler(@{});
}

- (void)session:(WCSession *)session didReceiveUserInfo:(NSDictionary<NSString *, id> *)userInfo {
  [self enqueue:userInfo[@"gs26_watch"]];
}

@end

void gs26_watch_start(void) {
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26WatchBridge shared] start];
  });
}

void gs26_watch_publish(const char *json, int urgent) {
  if (json == NULL) return;
  NSString *text = [NSString stringWithUTF8String:json];
  if (text == nil) return;
  [[GS26WatchBridge shared] publish:text urgent:urgent != 0];
}

// Copies the oldest queued watch message into buf (not NUL-terminated) and
// returns its length, or 0 when the queue is empty. Oversized entries are
// dropped.
int gs26_watch_take_incoming(char *buf, int cap) {
  if (buf == NULL || cap <= 0) return 0;
  GS26WatchBridge *bridge = [GS26WatchBridge shared];
  while (YES) {
    NSString *next;
    @synchronized(bridge) {
      if (bridge.incoming.count == 0) return 0;
      next = bridge.incoming.firstObject;
      [bridge.incoming removeObjectAtIndex:0];
    }
    NSData *data = [next dataUsingEncoding:NSUTF8StringEncoding];
    if (data.length == 0 || data.length > (NSUInteger)cap) continue;
    memcpy(buf, data.bytes, data.length);
    return (int)data.length;
  }
}

int gs26_watch_state(void) {
  return [[GS26WatchBridge shared] state];
}
