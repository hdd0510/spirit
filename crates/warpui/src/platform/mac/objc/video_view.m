#import <AVFoundation/AVFoundation.h>
#import <AppKit/AppKit.h>
#import <QuartzCore/QuartzCore.h>
#import <Quartz/Quartz.h>
#import <objc/runtime.h>

static char kVideoViewsKey;
static char kVideoWantedKey;

static NSMutableDictionary<NSString *, NSValue *> *savedVideoTimes(void) {
    static NSMutableDictionary *times;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
      times = [[NSMutableDictionary alloc] init];
    });
    return times;
}

static const CGFloat kVideoBarHeight = 40;

static NSString *formatVideoTime(double seconds) {
    if (!isfinite(seconds) || seconds < 0) seconds = 0;
    long total = (long)seconds;
    if (total >= 3600) {
        return [NSString stringWithFormat:@"%ld:%02ld:%02ld", total / 3600, (total / 60) % 60,
                                          total % 60];
    }
    return [NSString stringWithFormat:@"%ld:%02ld", total / 60, total % 60];
}

@interface WarpVideoButton : NSButton
@end

@implementation WarpVideoButton
- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    return YES;
}
- (BOOL)acceptsFirstResponder {
    return NO;
}
@end

@interface WarpVideoSlider : NSSlider
@end

@implementation WarpVideoSlider
- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    return YES;
}
- (BOOL)acceptsFirstResponder {
    return NO;
}
@end

@interface WarpVideoView : NSView
@property(nonatomic, copy) NSString *videoPath;
@property(nonatomic, retain) AVPlayer *player;
@property(nonatomic, retain) AVPlayerItem *observedItem;
@property(nonatomic, retain) AVPlayerLayer *videoLayer;
@property(nonatomic, retain) NSView *bar;
@property(nonatomic, retain) NSButton *playButton;
@property(nonatomic, retain) NSButton *muteButton;
@property(nonatomic, retain) NSButton *fullScreenButton;
@property(nonatomic, retain) NSSlider *slider;
@property(nonatomic, retain) NSTextField *timeLabel;
@property(nonatomic, retain) NSTextField *errorLabel;
@property(nonatomic, retain) id timeObserver;
@property(nonatomic) BOOL seeking;
@property(nonatomic) CMTime pendingSeek;
@end

@implementation WarpVideoView

- (NSButton *)makeButton:(NSString *)symbol label:(NSString *)label action:(SEL)action {
    WarpVideoButton *button = [[[WarpVideoButton alloc] initWithFrame:NSZeroRect] autorelease];
    button.bordered = NO;
    button.imagePosition = NSImageOnly;
    button.contentTintColor = NSColor.whiteColor;
    button.target = self;
    button.action = action;
    button.accessibilityLabel = label;
    [self setSymbol:symbol onButton:button];
    return button;
}

- (void)setSymbol:(NSString *)symbol onButton:(NSButton *)button {
    button.image = [NSImage imageWithSystemSymbolName:symbol
                             accessibilityDescription:button.accessibilityLabel];
}

- (NSTextField *)makeLabel {
    NSTextField *label = [NSTextField labelWithString:@""];
    label.textColor = NSColor.whiteColor;
    label.font = [NSFont monospacedDigitSystemFontOfSize:11 weight:NSFontWeightRegular];
    return label;
}

- (instancetype)initWithPath:(NSString *)path {
    self = [super initWithFrame:NSZeroRect];
    if (!self) return nil;
    self.videoPath = path;
    self.pendingSeek = kCMTimeInvalid;
    self.wantsLayer = YES;
    self.layer.backgroundColor = NSColor.blackColor.CGColor;
    self.layer.masksToBounds = YES;

    NSURL *url = [path hasPrefix:@"http://"] ? [NSURL URLWithString:path]
                                             : [NSURL fileURLWithPath:path];
    self.player = [AVPlayer playerWithURL:url];
    self.player.actionAtItemEnd = AVPlayerActionAtItemEndPause;
    self.videoLayer = [AVPlayerLayer playerLayerWithPlayer:self.player];
    self.videoLayer.videoGravity = AVLayerVideoGravityResizeAspect;
    [self.layer addSublayer:self.videoLayer];

    self.bar = [[[NSView alloc] initWithFrame:NSZeroRect] autorelease];
    self.bar.wantsLayer = YES;
    self.bar.layer.backgroundColor = [NSColor colorWithWhite:0.1 alpha:1].CGColor;
    [self addSubview:self.bar];
    self.playButton = [self makeButton:@"play.fill" label:@"Play" action:@selector(togglePlay)];
    self.muteButton = [self makeButton:@"speaker.wave.2.fill"
                                 label:@"Mute"
                                action:@selector(toggleMute)];
    self.fullScreenButton = [self makeButton:@"arrow.up.left.and.arrow.down.right"
                                       label:@"Full Screen"
                                      action:@selector(toggleFullScreen)];
    self.slider = [[[WarpVideoSlider alloc] initWithFrame:NSZeroRect] autorelease];
    self.slider.minValue = 0;
    self.slider.maxValue = 1;
    self.slider.continuous = YES;
    self.slider.target = self;
    self.slider.action = @selector(scrub);
    self.timeLabel = [self makeLabel];
    self.timeLabel.alignment = NSTextAlignmentRight;
    self.errorLabel = [self makeLabel];
    self.errorLabel.alignment = NSTextAlignmentCenter;
    self.errorLabel.hidden = YES;
    for (NSView *view in
         @[ self.playButton, self.slider, self.timeLabel, self.muteButton, self.fullScreenButton ]) {
        [self.bar addSubview:view];
    }
    [self addSubview:self.errorLabel];

    self.observedItem = self.player.currentItem;
    [self.observedItem addObserver:self forKeyPath:@"status" options:0 context:NULL];
    [self.player addObserver:self forKeyPath:@"rate" options:0 context:NULL];
    __block WarpVideoView *unretainedSelf = self;
    self.timeObserver = [self.player addPeriodicTimeObserverForInterval:CMTimeMake(1, 4)
                                                                  queue:dispatch_get_main_queue()
                                                             usingBlock:^(CMTime time) {
                                                               [unretainedSelf refreshTime];
                                                             }];
    NSValue *time = savedVideoTimes()[path];
    if (time) [self seekTo:time.CMTimeValue];
    return self;
}

- (BOOL)acceptsFirstResponder {
    return self.isInFullScreenMode;
}

- (BOOL)acceptsFirstMouse:(NSEvent *)event {
    return YES;
}

- (BOOL)mouseDownCanMoveWindow {
    return NO;
}

- (void)mouseDown:(NSEvent *)event {
    NSPoint point = [self convertPoint:event.locationInWindow fromView:nil];
    if (NSPointInRect(point, self.bar.frame)) return;
    if (event.clickCount == 2) {
        [self togglePlay];
        [self toggleFullScreen];
    } else {
        [self togglePlay];
    }
}

- (void)mouseUp:(NSEvent *)event {
}

- (void)mouseDragged:(NSEvent *)event {
}

- (void)rightMouseDown:(NSEvent *)event {
}

- (void)scrollWheel:(NSEvent *)event {
}

- (void)keyDown:(NSEvent *)event {
    if (event.keyCode == 53) {
        [self exitVideoFullScreen];
    } else if (event.keyCode == 49) {
        [self togglePlay];
    } else {
        [super keyDown:event];
    }
}

- (void)layout {
    [super layout];
    CGFloat width = NSWidth(self.bounds);
    CGFloat height = NSHeight(self.bounds);
    CGFloat barHeight = MIN(kVideoBarHeight, height);
    [CATransaction begin];
    [CATransaction setDisableActions:YES];
    self.videoLayer.frame = CGRectMake(0, barHeight, width, MAX(0, height - barHeight));
    [CATransaction commit];
    self.bar.frame = NSMakeRect(0, 0, width, barHeight);
    CGFloat size = 28;
    CGFloat y = (barHeight - size) / 2;
    self.playButton.frame = NSMakeRect(8, y, size, size);
    self.fullScreenButton.frame = NSMakeRect(width - 8 - size, y, size, size);
    self.muteButton.frame = NSMakeRect(width - 8 - size * 2, y, size, size);
    BOOL showTime = width > 320;
    CGFloat timeWidth = showTime ? 110 : 0;
    self.timeLabel.hidden = !showTime;
    self.timeLabel.frame =
        NSMakeRect(width - 16 - size * 2 - timeWidth, (barHeight - 16) / 2, timeWidth, 16);
    CGFloat sliderX = 16 + size;
    self.slider.frame = NSMakeRect(
        sliderX, y, MAX(0, width - 16 - size * 2 - timeWidth - 8 - sliderX), size);
    self.errorLabel.frame = NSMakeRect(8, barHeight + (height - barHeight) / 2, MAX(0, width - 16), 18);
}

- (void)setFrameSize:(NSSize)size {
    [super setFrameSize:size];
    self.needsLayout = YES;
}

- (double)duration {
    double seconds = CMTimeGetSeconds(self.player.currentItem.duration);
    return isfinite(seconds) ? seconds : 0;
}

- (void)refreshTime {
    double duration = [self duration];
    double now = CMTimeGetSeconds(self.player.currentTime);
    if (!self.seeking && duration > 0) self.slider.doubleValue = now / duration;
    self.timeLabel.stringValue = [NSString
        stringWithFormat:@"%@ / %@", formatVideoTime(now), formatVideoTime(duration)];
}

- (void)observeValueForKeyPath:(NSString *)keyPath
                      ofObject:(id)object
                        change:(NSDictionary *)change
                       context:(void *)context {
    if (!NSThread.isMainThread) {
        dispatch_async(dispatch_get_main_queue(), ^{
          [self refreshState];
        });
        return;
    }
    [self refreshState];
}

- (void)refreshState {
    BOOL playing = self.player.rate > 0;
    self.playButton.accessibilityLabel = playing ? @"Pause" : @"Play";
    [self setSymbol:playing ? @"pause.fill" : @"play.fill" onButton:self.playButton];
    BOOL failed = self.player.currentItem.status == AVPlayerItemStatusFailed;
    self.errorLabel.hidden = !failed;
    if (failed) {
        self.errorLabel.stringValue =
            @"This video can't be played here. Use \"Open in default app\" from the pane menu.";
    }
    [self refreshTime];
}

- (void)togglePlay {
    if (self.player.rate > 0) {
        [self.player pause];
        return;
    }
    double duration = [self duration];
    if (duration > 0 && CMTimeGetSeconds(self.player.currentTime) >= duration - 0.05) {
        [self.player seekToTime:kCMTimeZero
                toleranceBefore:kCMTimeZero
                 toleranceAfter:kCMTimeZero];
    }
    [self.player play];
}

- (void)toggleMute {
    self.player.muted = !self.player.muted;
    self.muteButton.accessibilityLabel = self.player.muted ? @"Unmute" : @"Mute";
    [self setSymbol:self.player.muted ? @"speaker.slash.fill" : @"speaker.wave.2.fill"
           onButton:self.muteButton];
}

- (void)scrub {
    double duration = [self duration];
    if (duration <= 0) return;
    [self seekTo:CMTimeMakeWithSeconds(self.slider.doubleValue * duration, 600)];
}

- (void)seekTo:(CMTime)time {
    if (self.seeking) {
        self.pendingSeek = time;
        return;
    }
    self.seeking = YES;
    CMTime tolerance = CMTimeMake(1, 20);
    [self.player seekToTime:time
            toleranceBefore:tolerance
             toleranceAfter:tolerance
          completionHandler:^(BOOL finished) {
            dispatch_async(dispatch_get_main_queue(), ^{
              self.seeking = NO;
              CMTime pending = self.pendingSeek;
              self.pendingSeek = kCMTimeInvalid;
              if (CMTIME_IS_VALID(pending)) {
                  [self seekTo:pending];
              } else {
                  [self refreshTime];
              }
            });
          }];
}

- (void)toggleFullScreen {
    if (self.isInFullScreenMode) {
        [self exitVideoFullScreen];
        return;
    }
    NSScreen *screen = self.window.screen ?: NSScreen.mainScreen;
    if (!screen) return;
    [self enterFullScreenMode:screen withOptions:@{NSFullScreenModeAllScreens : @NO}];
    [self.window makeFirstResponder:self];
}

- (void)exitVideoFullScreen {
    if (!self.isInFullScreenMode) return;
    [self exitFullScreenModeWithOptions:nil];
    [self.window makeFirstResponder:self.superview];
}

- (void)windowWillClose:(NSNotification *)notification {
    if (!self.isInFullScreenMode) [self.player pause];
}

- (void)viewDidMoveToWindow {
    [super viewDidMoveToWindow];
    NSNotificationCenter *center = [NSNotificationCenter defaultCenter];
    [center removeObserver:self name:NSWindowWillCloseNotification object:nil];
    if (self.window) {
        [center addObserver:self
                   selector:@selector(windowWillClose:)
                       name:NSWindowWillCloseNotification
                     object:self.window];
    }
}

- (void)stopPlayer {
    if (!self.player) return;
    [self.player pause];
    if (self.timeObserver) [self.player removeTimeObserver:self.timeObserver];
    self.timeObserver = nil;
    [self.observedItem removeObserver:self forKeyPath:@"status"];
    [self.player removeObserver:self forKeyPath:@"rate"];
    self.observedItem = nil;
    self.videoLayer.player = nil;
    self.player = nil;
}

- (void)closeVideo {
    [self exitVideoFullScreen];
    CMTime time = self.player.currentTime;
    if (CMTIME_IS_NUMERIC(time)) {
        savedVideoTimes()[self.videoPath] = [NSValue valueWithCMTime:time];
    }
    [self stopPlayer];
    [self removeFromSuperview];
}

- (void)dealloc {
    [[NSNotificationCenter defaultCenter] removeObserver:self];
    [self stopPlayer];
    [_videoPath release];
    [_videoLayer release];
    [_bar release];
    [_playButton release];
    [_muteButton release];
    [_fullScreenButton release];
    [_slider release];
    [_timeLabel release];
    [_errorLabel release];
    [super dealloc];
}

@end

static BOOL isVideoPath(NSString *path) {
    if ([path hasPrefix:@"http://"]) return YES;
    NSSet *videoExtensions = [NSSet setWithArray:@[
        @"mp4", @"m4v", @"mov", @"mkv", @"webm", @"avi", @"wmv", @"flv", @"mpg", @"mpeg", @"ogv",
        @"3gp"
    ]];
    return [videoExtensions containsObject:path.pathExtension.lowercaseString];
}

static NSDate *modificationDate(NSString *path) {
    NSDictionary *attributes = [NSFileManager.defaultManager attributesOfItemAtPath:path error:nil];
    return attributes.fileModificationDate ?: [NSDate distantPast];
}

@interface WarpDocumentView : NSView
@property(nonatomic, retain) QLPreviewView *preview;
@property(nonatomic, copy) NSString *documentPath;
@property(nonatomic, retain) NSDate *shownModificationDate;
- (instancetype)initWithPath:(NSString *)path;
- (void)refreshIfChanged;
- (void)closeVideo;
@end

@implementation WarpDocumentView

- (instancetype)initWithPath:(NSString *)path {
    self = [super initWithFrame:NSZeroRect];
    if (!self) return nil;
    self.documentPath = path;
    self.wantsLayer = YES;
    self.preview = [[[QLPreviewView alloc] initWithFrame:NSZeroRect
                                                   style:QLPreviewViewStyleNormal] autorelease];
    self.preview.autoresizingMask = NSViewWidthSizable | NSViewHeightSizable;
    self.preview.shouldCloseWithWindow = NO;
    [self addSubview:self.preview];
    self.shownModificationDate = modificationDate(path);
    self.preview.previewItem = [NSURL fileURLWithPath:path];
    return self;
}

- (void)setFrame:(NSRect)frame {
    [super setFrame:frame];
    self.preview.frame = self.bounds;
}

- (void)refreshIfChanged {
    NSDate *current = modificationDate(self.documentPath);
    if ([current isEqualToDate:self.shownModificationDate]) return;
    self.shownModificationDate = current;
    self.preview.previewItem = [NSURL fileURLWithPath:self.documentPath];
    [self.preview refreshPreviewItem];
}

- (void)closeVideo {
    [self.preview close];
    [self removeFromSuperview];
}

- (void)dealloc {
    [_preview release];
    [_documentPath release];
    [_shownModificationDate release];
    [super dealloc];
}

@end

BOOL warp_video_owns_event(NSWindow *window, NSEvent *event) {
    NSView *content = window.contentView;
    if (!content) return NO;
    NSPoint point = [content.superview convertPoint:event.locationInWindow fromView:nil];
    for (NSView *view = [content hitTest:point]; view && view != content; view = view.superview) {
        if ([view isKindOfClass:[WarpVideoView class]] ||
            [view isKindOfClass:[WarpDocumentView class]]) {
            return YES;
        }
    }
    return NO;
}

void warp_sync_video_views(NSView *host, const char *const *paths, const CGRect *frames,
                           const BOOL *covered, NSUInteger count) {
    NSMutableDictionary<NSString *, id> *views = objc_getAssociatedObject(host, &kVideoViewsKey);
    if (!views && count == 0) return;
    NSMutableDictionary<NSString *, NSDictionary *> *wanted = [NSMutableDictionary dictionary];
    for (NSUInteger i = 0; i < count; i++) {
        NSString *path = [NSString stringWithUTF8String:paths[i]];
        if (!path) continue;
        NSString *key = path;
        for (NSUInteger copy = 2; wanted[key]; copy++) {
            key = [NSString stringWithFormat:@"%@#%lu", path, (unsigned long)copy];
        }
        wanted[key] = @{
            @"path" : path,
            @"frame" : [NSValue valueWithRect:frames[i]],
            @"covered" : @(covered[i]),
            @"hostHeight" : @(NSHeight(host.bounds)),
            @"modified" : isVideoPath(path) ? [NSDate distantPast] : modificationDate(path),
        };
    }
    if ([objc_getAssociatedObject(host, &kVideoWantedKey) isEqual:wanted]) return;
    objc_setAssociatedObject(host, &kVideoWantedKey, wanted, OBJC_ASSOCIATION_RETAIN_NONATOMIC);
    dispatch_async(dispatch_get_main_queue(), ^{
      NSMutableDictionary<NSString *, id> *current =
          objc_getAssociatedObject(host, &kVideoViewsKey);
      if (!current) {
          current = [NSMutableDictionary dictionary];
          objc_setAssociatedObject(host, &kVideoViewsKey, current,
                                   OBJC_ASSOCIATION_RETAIN_NONATOMIC);
      }
      for (NSString *key in current.allKeys) {
          if (wanted[key]) continue;
          [current[key] closeVideo];
          [current removeObjectForKey:key];
      }
      for (NSString *key in wanted) {
          NSDictionary *info = wanted[key];
          NSView *view = current[key];
          if (!view) {
              NSString *path = info[@"path"];
              view = isVideoPath(path)
                         ? (NSView *)[[[WarpVideoView alloc] initWithPath:path] autorelease]
                         : (NSView *)[[[WarpDocumentView alloc] initWithPath:path] autorelease];
              current[key] = view;
              [host addSubview:view];
          } else if ([view isKindOfClass:[WarpDocumentView class]]) {
              [(WarpDocumentView *)view refreshIfChanged];
          }
          NSRect frame = [info[@"frame"] rectValue];
          frame.origin.y = NSHeight(host.bounds) - NSMaxY(frame);
          if (view.superview == host && !NSEqualRects(view.frame, frame)) view.frame = frame;
          BOOL hidden = [info[@"covered"] boolValue];
          if (view.hidden != hidden) view.hidden = hidden;
      }
    });
}
