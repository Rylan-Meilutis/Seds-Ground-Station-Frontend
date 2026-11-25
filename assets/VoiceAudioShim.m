// Crew voice audio output on iOS (Rust: src/native_voice_audio.rs).
//
// The operator chooses the output on the voice page: Phone (the earpiece, or
// the connected headset, like a call) or Speaker. WebKit owns the audio
// session while the microphone is open (it sets play-and-record); this shim
// does not set a category or mode itself, which would fight WebKit. It
// reports the session after every route change, interruption, media-services
// reset, return to the foreground and change of choice, and applies the port
// override Rust answers with: 1 overrideOutputAudioPort(.speaker), 2 .none
// (undo our speaker override), 0 nothing. Re-applying after each event keeps
// the choice when a headset arrives or an interruption ends. Each report also
// lands in the app's debug log ([voice-audio] ...).

#import <AVFoundation/AVFoundation.h>
#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>

typedef int (*GS26VoiceAudioCallback)(const char *json);

static GS26VoiceAudioCallback g_voice_audio_callback;
static BOOL g_voice_audio_speaker_override;

static NSArray<NSString *> *GS26PortTypes(NSArray<AVAudioSessionPortDescription *> *ports) {
    NSMutableArray<NSString *> *types = [NSMutableArray arrayWithCapacity:ports.count];
    for (AVAudioSessionPortDescription *port in ports) {
        [types addObject:port.portType ?: @"?"];
    }
    return types;
}

static NSString *GS26RecordPermission(void) {
    if (@available(iOS 17.0, *)) {
        switch (AVAudioApplication.sharedInstance.recordPermission) {
            case AVAudioApplicationRecordPermissionGranted: return @"granted";
            case AVAudioApplicationRecordPermissionDenied: return @"denied";
            default: return @"undetermined";
        }
    }
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    switch (AVAudioSession.sharedInstance.recordPermission) {
        case AVAudioSessionRecordPermissionGranted: return @"granted";
        case AVAudioSessionRecordPermissionDenied: return @"denied";
        default: return @"undetermined";
    }
#pragma clang diagnostic pop
}

static NSString *GS26RouteChangeReason(NSNumber *reason) {
    switch ((AVAudioSessionRouteChangeReason)reason.unsignedIntegerValue) {
        case AVAudioSessionRouteChangeReasonNewDeviceAvailable: return @"NewDeviceAvailable";
        case AVAudioSessionRouteChangeReasonOldDeviceUnavailable: return @"OldDeviceUnavailable";
        case AVAudioSessionRouteChangeReasonCategoryChange: return @"CategoryChange";
        case AVAudioSessionRouteChangeReasonOverride: return @"Override";
        case AVAudioSessionRouteChangeReasonWakeFromSleep: return @"WakeFromSleep";
        case AVAudioSessionRouteChangeReasonNoSuitableRouteForCategory: return @"NoSuitableRouteForCategory";
        case AVAudioSessionRouteChangeReasonRouteConfigurationChange: return @"RouteConfigurationChange";
        default: return @"Unknown";
    }
}

static void GS26VoiceAudioReport(NSString *event, NSString *detail) {
    if (g_voice_audio_callback == NULL) {
        return;
    }
    AVAudioSession *session = AVAudioSession.sharedInstance;
    AVAudioSessionRouteDescription *route = session.currentRoute;
    NSDictionary *report = @{
        @"event": event,
        @"detail": detail ?: @"",
        @"category": session.category ?: @"",
        @"mode": session.mode ?: @"",
        @"outputs": GS26PortTypes(route.outputs),
        @"inputs": GS26PortTypes(route.inputs),
        @"record_permission": GS26RecordPermission(),
        @"sample_rate": @(session.sampleRate),
        @"speaker_override": @(g_voice_audio_speaker_override),
    };
    NSData *data = [NSJSONSerialization dataWithJSONObject:report options:0 error:nil];
    if (data == nil) {
        return;
    }
    NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
    int action = g_voice_audio_callback(json.UTF8String);
    if (action == 0) {
        return;
    }
    NSError *error = nil;
    AVAudioSessionPortOverride port = action == 1 ? AVAudioSessionPortOverrideSpeaker : AVAudioSessionPortOverrideNone;
    if ([session overrideOutputAudioPort:port error:&error]) {
        g_voice_audio_speaker_override = action == 1;
        NSLog(@"[voice-audio] output override: %@", action == 1 ? @"speaker" : @"none (earpiece or headset)");
    } else {
        NSLog(@"[voice-audio] could not apply the output override %d: %@", action, error);
    }
}

// The operator changed the choice: report now so Rust answers with the override.
void gs26_voice_audio_refresh(void) {
    void (^refresh)(void) = ^{
        GS26VoiceAudioReport(@"choice", nil);
    };
    if (NSThread.isMainThread) {
        refresh();
    } else {
        dispatch_async(dispatch_get_main_queue(), refresh);
    }
}

void gs26_voice_audio_install(GS26VoiceAudioCallback callback) {
    void (^install)(void) = ^{
        if (g_voice_audio_callback != NULL) {
            return;
        }
        g_voice_audio_callback = callback;
        NSNotificationCenter *center = NSNotificationCenter.defaultCenter;
        NSOperationQueue *main = NSOperationQueue.mainQueue;
        [center addObserverForName:AVAudioSessionRouteChangeNotification object:nil queue:main
                        usingBlock:^(NSNotification *note) {
            NSString *reason = GS26RouteChangeReason(note.userInfo[AVAudioSessionRouteChangeReasonKey]);
            // A category change (WebKit closing the microphone) drops the override.
            if (![reason isEqualToString:@"Override"]) {
                g_voice_audio_speaker_override = NO;
            }
            GS26VoiceAudioReport(@"route", reason);
        }];
        [center addObserverForName:AVAudioSessionInterruptionNotification object:nil queue:main
                        usingBlock:^(NSNotification *note) {
            NSNumber *type = note.userInfo[AVAudioSessionInterruptionTypeKey];
            BOOL began = type.unsignedIntegerValue == AVAudioSessionInterruptionTypeBegan;
            GS26VoiceAudioReport(@"interruption", began ? @"began" : @"ended");
        }];
        [center addObserverForName:AVAudioSessionMediaServicesWereResetNotification object:nil queue:main
                        usingBlock:^(NSNotification *note) {
            (void)note;
            g_voice_audio_speaker_override = NO;
            GS26VoiceAudioReport(@"reset", @"media services were reset");
        }];
        [center addObserverForName:UIApplicationDidBecomeActiveNotification object:nil queue:main
                        usingBlock:^(NSNotification *note) {
            (void)note;
            GS26VoiceAudioReport(@"active", nil);
        }];
        GS26VoiceAudioReport(@"install", nil);
    };
    if (NSThread.isMainThread) {
        install();
    } else {
        dispatch_async(dispatch_get_main_queue(), install);
    }
}
