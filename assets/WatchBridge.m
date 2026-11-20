// Phone side of the Apple Watch companion (iOS only). Rust publishes snapshot
// JSON and polls watch messages through the C functions below; this file only
// moves strings between WCSession and a small queue. It never interprets them.
//
// Direct-link provisioning (station address + scoped watch token, key
// "gs26_direct") goes out with transferUserInfo only: iOS delivers it even
// after this app is suspended (or when the watch app next launches), and the
// watch moves the token into its keychain. It is kept out of the application
// context so no second copy of the token is stored.
//
// Map background (docs/watch-companion.md "Map background"): the tile-set
// manifest goes out with transferUserInfo (key "gs26_map", replacing an
// older queued one) and each tile with transferFile (metadata
// {"gs26_tile": "z/x/y", "set": id}). A new manifest cancels queued file
// transfers for tiles that left the set.
#import <Foundation/Foundation.h>
#import <WatchConnectivity/WatchConnectivity.h>

static const NSUInteger kMaxQueued = 32;
static const NSUInteger kMaxMessageBytes = 32 * 1024;

@interface GS26WatchBridge : NSObject <WCSessionDelegate>
@property(nonatomic, strong) NSMutableArray<NSString *> *incoming;
// Crew voice relay from the watch (key gs26_watch_voice), drained every 20 ms
// by the phone's relay while it runs; never mixed with the settings queue.
@property(nonatomic, strong) NSMutableArray<NSString *> *voiceIncoming;
@property(nonatomic, copy) NSString *pendingContext;
@property(nonatomic, copy) NSString *provision;
@property(nonatomic, assign) BOOL provisionPending;
@property(nonatomic, copy) NSString *mapManifest;
@property(nonatomic, assign) BOOL mapPending;
@property(nonatomic, strong) NSMutableDictionary<NSString *, NSDictionary *> *pendingTiles;
@end

@implementation GS26WatchBridge

+ (instancetype)shared {
  static GS26WatchBridge *bridge;
  static dispatch_once_t once;
  dispatch_once(&once, ^{
    bridge = [GS26WatchBridge new];
    bridge.incoming = [NSMutableArray array];
    bridge.voiceIncoming = [NSMutableArray array];
    bridge.pendingTiles = [NSMutableDictionary dictionary];
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
  [self flushProvision:session];
  NSError *error = nil;
  // Latest state survives the watch app being closed or out of range.
  [session updateApplicationContext:@{@"gs26" : json} error:&error];
  if (urgent && session.isReachable) {
    [session sendMessage:@{@"gs26" : json} replyHandler:nil errorHandler:nil];
  }
}

- (void)setProvisionJSON:(NSString *)json {
  @synchronized(self) {
    if ([self.provision isEqualToString:json]) return;
    self.provision = json;
    self.provisionPending = YES;
  }
  if (![WCSession isSupported]) return;
  [self flushProvision:[WCSession defaultSession]];
}

// Queues the latest provisioning once the session can deliver it, replacing
// an older one still waiting in the transfer queue.
- (void)flushProvision:(WCSession *)session {
  NSString *provision;
  @synchronized(self) {
    if (!self.provisionPending || !self.provision) return;
    if (session.activationState != WCSessionActivationStateActivated || !session.isPaired ||
        !session.isWatchAppInstalled) {
      return;
    }
    provision = self.provision;
    self.provisionPending = NO;
  }
  for (WCSessionUserInfoTransfer *transfer in session.outstandingUserInfoTransfers) {
    if (transfer.userInfo[@"gs26_direct"] != nil) [transfer cancel];
  }
  [session transferUserInfo:@{@"gs26_direct" : provision}];
}

#pragma mark Map background

- (BOOL)canTransfer:(WCSession *)session {
  return session.activationState == WCSessionActivationStateActivated && session.isPaired &&
         session.isWatchAppInstalled;
}

- (void)setMapJSON:(NSString *)json {
  @synchronized(self) {
    if ([self.mapManifest isEqualToString:json]) return;
    self.mapManifest = json;
    self.mapPending = YES;
  }
  if (![WCSession isSupported]) return;
  [self flushMap:[WCSession defaultSession]];
}

// Sends the latest manifest and cancels queued tiles outside it.
- (void)flushMap:(WCSession *)session {
  NSString *manifest;
  @synchronized(self) {
    if (!self.mapPending || !self.mapManifest || ![self canTransfer:session]) return;
    manifest = self.mapManifest;
    self.mapPending = NO;
  }
  NSSet *keep = nil;
  NSData *data = [manifest dataUsingEncoding:NSUTF8StringEncoding];
  NSDictionary *parsed = data ? [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] : nil;
  if ([parsed isKindOfClass:[NSDictionary class]] && [parsed[@"tiles"] isKindOfClass:[NSArray class]]) {
    keep = [NSSet setWithArray:parsed[@"tiles"]];
  }
  for (WCSessionUserInfoTransfer *transfer in session.outstandingUserInfoTransfers) {
    if (transfer.userInfo[@"gs26_map"] != nil) [transfer cancel];
  }
  if (keep) {
    for (WCSessionFileTransfer *transfer in session.outstandingFileTransfers) {
      NSString *key = transfer.file.metadata[@"gs26_tile"];
      if (key && ![keep containsObject:key]) [transfer cancel];
    }
  }
  [session transferUserInfo:@{@"gs26_map" : manifest}];
}

- (void)sendTile:(NSString *)key path:(NSString *)path set:(NSString *)set {
  if (![WCSession isSupported]) return;
  WCSession *session = [WCSession defaultSession];
  NSDictionary *metadata = @{@"gs26_tile" : key, @"set" : set};
  if (![self canTransfer:session]) {
    @synchronized(self) { self.pendingTiles[key] = @{@"path" : path, @"metadata" : metadata}; }
    return;
  }
  for (WCSessionFileTransfer *transfer in session.outstandingFileTransfers) {
    if ([transfer.file.metadata[@"gs26_tile"] isEqualToString:key]) return;
  }
  if (![[NSFileManager defaultManager] fileExistsAtPath:path]) return;
  [session transferFile:[NSURL fileURLWithPath:path] metadata:metadata];
}

- (void)flushTiles:(WCSession *)session {
  if (![self canTransfer:session]) return;
  NSDictionary *pending;
  @synchronized(self) {
    pending = [self.pendingTiles copy];
    [self.pendingTiles removeAllObjects];
  }
  [pending enumerateKeysAndObjectsUsingBlock:^(NSString *key, NSDictionary *entry, BOOL *stop) {
    [self sendTile:key path:entry[@"path"] set:entry[@"metadata"][@"set"]];
  }];
}

- (void)session:(WCSession *)session
    didFinishFileTransfer:(WCSessionFileTransfer *)fileTransfer
                    error:(NSError *)error {
  if (error && error.code != WCErrorCodeTransferTimedOut) {
    NSLog(@"GS26 watch: tile %@ transfer failed: %@", fileTransfer.file.metadata[@"gs26_tile"], error);
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
  if (activationState == WCSessionActivationStateActivated) {
    [self flushProvision:session];
    [self flushMap:session];
    [self flushTiles:session];
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
  [self flushProvision:session];
  [self flushMap:session];
  [self flushTiles:session];
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

- (void)enqueueVoice:(id)value {
  if (![value isKindOfClass:[NSString class]]) return;
  NSString *text = value;
  if ([text lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > kMaxMessageBytes) return;
  @synchronized(self) {
    // About a second of relayed audio at most; older audio is useless.
    if (self.voiceIncoming.count >= 64) [self.voiceIncoming removeObjectAtIndex:0];
    [self.voiceIncoming addObject:text];
  }
}

// Crew voice relay to the watch: only while reachable, never queued (late
// audio is worthless).
- (void)sendVoice:(NSString *)json {
  WCSession *session = [WCSession defaultSession];
  if (session.activationState != WCSessionActivationStateActivated || !session.reachable) return;
  [session sendMessage:@{@"gs26_voice" : json} replyHandler:nil errorHandler:nil];
}

- (void)session:(WCSession *)session didReceiveMessage:(NSDictionary<NSString *, id> *)message {
  [self enqueue:message[@"gs26_watch"]];
  [self enqueueVoice:message[@"gs26_watch_voice"]];
}

- (void)session:(WCSession *)session
    didReceiveMessage:(NSDictionary<NSString *, id> *)message
         replyHandler:(void (^)(NSDictionary<NSString *, id> *))replyHandler {
  [self enqueue:message[@"gs26_watch"]];
  [self enqueueVoice:message[@"gs26_watch_voice"]];
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

// Same contract as gs26_watch_take_incoming, for the voice relay queue.
int gs26_watch_take_voice(char *buf, int cap) {
  if (buf == NULL || cap <= 0) return 0;
  GS26WatchBridge *bridge = [GS26WatchBridge shared];
  while (YES) {
    NSString *next;
    @synchronized(bridge) {
      if (bridge.voiceIncoming.count == 0) return 0;
      next = bridge.voiceIncoming.firstObject;
      [bridge.voiceIncoming removeObjectAtIndex:0];
    }
    NSData *data = [next dataUsingEncoding:NSUTF8StringEncoding];
    if (data.length == 0 || data.length > (NSUInteger)cap) continue;
    memcpy(buf, data.bytes, data.length);
    return (int)data.length;
  }
}

void gs26_watch_send_voice(const char *json) {
  if (json == NULL) return;
  NSString *text = [NSString stringWithUTF8String:json];
  if (text == nil || [text lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > kMaxMessageBytes) return;
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26WatchBridge shared] sendVoice:text];
  });
}

void gs26_watch_provision(const char *json) {
  if (json == NULL) return;
  NSString *text = [NSString stringWithUTF8String:json];
  if (text == nil || [text lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > kMaxMessageBytes) return;
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26WatchBridge shared] setProvisionJSON:text];
  });
}

void gs26_watch_send_map(const char *json) {
  if (json == NULL) return;
  NSString *text = [NSString stringWithUTF8String:json];
  if (text == nil || [text lengthOfBytesUsingEncoding:NSUTF8StringEncoding] > kMaxMessageBytes) return;
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26WatchBridge shared] setMapJSON:text];
  });
}

void gs26_watch_send_tile(const char *key, const char *path, const char *set) {
  if (key == NULL || path == NULL || set == NULL) return;
  NSString *k = [NSString stringWithUTF8String:key];
  NSString *p = [NSString stringWithUTF8String:path];
  NSString *s = [NSString stringWithUTF8String:set];
  if (k == nil || p == nil || s == nil) return;
  dispatch_async(dispatch_get_main_queue(), ^{
    [[GS26WatchBridge shared] sendTile:k path:p set:s];
  });
}

int gs26_watch_state(void) {
  return [[GS26WatchBridge shared] state];
}
