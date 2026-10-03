#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <WebKit/WebKit.h>
#import <stdint.h>
#import <stdlib.h>
#import <string.h>

static NSString *bridge_script(void)
{
    return @"window.engine={post:function(text){window.webkit.messageHandlers.engine.postMessage(String(text));}};";
}

static void unpremultiply(uint8_t *pixels, size_t count)
{
    size_t idx = 0;

    while (idx < count)
    {
        uint8_t *px = pixels + idx * 4;
        uint8_t alpha = px[3];

        if (alpha == 0)
        {
            px[0] = 0;
            px[1] = 0;
            px[2] = 0;
        }
        else if (alpha != 255)
        {
            px[0] = (uint8_t)(((unsigned)px[0] * 255u) / alpha);
            px[1] = (uint8_t)(((unsigned)px[1] * 255u) / alpha);
            px[2] = (uint8_t)(((unsigned)px[2] * 255u) / alpha);
        }

        idx += 1;
    }
}

static NSString *ns_text(const char *text)
{
    if (text == NULL)
    {
        return @"";
    }

    NSString *string = [NSString stringWithUTF8String:text];

    if (string == nil)
    {
        return @"";
    }

    return string;
}

static void ensure_app(void)
{
    [NSApplication sharedApplication];
}

@interface EngineWebFrame : NSObject
@property (nonatomic, assign) uint64_t viewId;
@property (nonatomic, assign) uint32_t width;
@property (nonatomic, assign) uint32_t height;
@property (nonatomic, strong) NSData *bytes;
@end

@implementation EngineWebFrame
@end

@interface EngineWebNote : NSObject
@property (nonatomic, assign) uint64_t viewId;
@property (nonatomic, copy) NSString *text;
@end

@implementation EngineWebNote
@end

@class EngineWebHost;

@interface EngineWebSlot : NSObject <WKNavigationDelegate, WKScriptMessageHandler>
@property (nonatomic, weak) EngineWebHost *host;
@property (nonatomic, assign) uint64_t viewId;
@property (nonatomic, strong) NSWindow *window;
@property (nonatomic, strong) WKWebView *web;
@property (nonatomic, assign) uint32_t width;
@property (nonatomic, assign) uint32_t height;
@property (nonatomic, assign) int buttons;
@property (nonatomic, assign) float lastX;
@property (nonatomic, assign) float lastY;
@property (nonatomic, assign) BOOL live;
@property (nonatomic, assign) BOOL boost;
@property (nonatomic, assign) BOOL busy;
@property (nonatomic, assign) BOOL again;
- (void)requestSnapshot;
@end

@interface EngineWebHost : NSObject
@property (nonatomic, strong) NSMutableArray<EngineWebSlot *> *slots;
@property (nonatomic, strong) NSMutableArray<EngineWebFrame *> *frames;
@property (nonatomic, strong) NSMutableArray<EngineWebNote *> *notes;
@property (nonatomic, assign) uint64_t nextId;
- (EngineWebSlot *)slotFor:(uint64_t)viewId;
- (void)pushSnapshot:(NSImage *)image slot:(EngineWebSlot *)slot;
- (void)pushMessage:(uint64_t)viewId text:(NSString *)text;
- (void)shutdown;
@end

@implementation EngineWebSlot

- (NSPoint)pointForX:(float)x y:(float)y
{
    return NSMakePoint((CGFloat)x, (CGFloat)self.height - (CGFloat)y);
}

- (NSEvent *)mouseEventOfType:(NSEventType)type at:(NSPoint)where
{
    return [NSEvent mouseEventWithType:type
                               location:where
                          modifierFlags:0
                              timestamp:[NSProcessInfo processInfo].systemUptime
                           windowNumber:self.window.windowNumber
                                context:nil
                            eventNumber:0
                             clickCount:1
                               pressure:1];
}

- (void)requestSnapshot
{
    if (self.web == nil || self.width == 0 || self.height == 0)
    {
        return;
    }

    if (self.busy)
    {
        self.again = YES;

        return;
    }

    self.busy = YES;
    WKSnapshotConfiguration *config = [WKSnapshotConfiguration new];
    config.rect = NSMakeRect(0, 0, self.width, self.height);
    config.snapshotWidth = @(self.width);
    config.afterScreenUpdates = NO;
    __weak EngineWebHost *host = self.host;
    uint64_t viewId = self.viewId;
    [self.web takeSnapshotWithConfiguration:config completionHandler:^(NSImage *image, NSError *error) {
        EngineWebHost *strong = host;

        if (strong == nil)
        {
            return;
        }

        EngineWebSlot *slot = [strong slotFor:viewId];

        if (slot == nil)
        {
            return;
        }

        slot.busy = NO;

        if (image != nil)
        {
            [strong pushSnapshot:image slot:slot];
        }
        else if (error != nil)
        {
            NSLog(@"[webview] %@", error.localizedDescription);
        }

        if (slot.again)
        {
            slot.again = NO;
            [slot requestSnapshot];
        }
    }];
}

- (void)userContentController:(WKUserContentController *)controller didReceiveScriptMessage:(WKScriptMessage *)message
{
    (void)controller;
    NSString *text = @"";

    if ([message.body isKindOfClass:[NSString class]])
    {
        text = (NSString *)message.body;
    }
    else if (message.body != nil)
    {
        text = [message.body description];
    }

    [self.host pushMessage:self.viewId text:text];
}

- (void)webView:(WKWebView *)webView didCommitNavigation:(WKNavigation *)navigation
{
    (void)webView;
    (void)navigation;
    self.live = YES;
    self.boost = YES;
}

- (void)webView:(WKWebView *)webView didFinishNavigation:(WKNavigation *)navigation
{
    (void)webView;
    (void)navigation;
    self.live = YES;
    self.boost = YES;
}

@end

@implementation EngineWebHost

- (instancetype)init
{
    self = [super init];

    if (self == nil)
    {
        return nil;
    }

    _slots = [NSMutableArray array];
    _frames = [NSMutableArray array];
    _notes = [NSMutableArray array];
    _nextId = 1;

    return self;
}

- (EngineWebSlot *)slotFor:(uint64_t)viewId
{
    NSUInteger idx = 0;

    while (idx < self.slots.count)
    {
        EngineWebSlot *slot = self.slots[idx];

        if (slot.viewId == viewId)
        {
            return slot;
        }

        idx += 1;
    }

    return nil;
}

- (void)pushMessage:(uint64_t)viewId text:(NSString *)text
{
    EngineWebNote *note = [EngineWebNote new];
    note.viewId = viewId;
    note.text = text == nil ? @"" : text;
    [self.notes addObject:note];
}

- (void)pushSnapshot:(NSImage *)image slot:(EngineWebSlot *)slot
{
    CGImageRef cg = [image CGImageForProposedRect:NULL context:nil hints:nil];

    if (cg == NULL)
    {
        return;
    }

    size_t width = CGImageGetWidth(cg);
    size_t height = CGImageGetHeight(cg);

    if (width == 0 || height == 0 || width > 8192 || height > 8192)
    {
        return;
    }

    size_t count = width * height;
    uint8_t *pixels = (uint8_t *)calloc(count, 4);

    if (pixels == NULL)
    {
        return;
    }

    CGColorSpaceRef space = CGColorSpaceCreateDeviceRGB();
    CGContextRef ctx = CGBitmapContextCreate(
        pixels,
        width,
        height,
        8,
        width * 4,
        space,
        (CGBitmapInfo)kCGImageAlphaPremultipliedLast | (CGBitmapInfo)kCGBitmapByteOrder32Big);
    CGColorSpaceRelease(space);

    if (ctx == NULL)
    {
        free(pixels);

        return;
    }

    CGContextDrawImage(ctx, CGRectMake(0, 0, (CGFloat)width, (CGFloat)height), cg);
    CGContextRelease(ctx);
    unpremultiply(pixels, count);

    EngineWebFrame *frame = [EngineWebFrame new];
    frame.viewId = slot.viewId;
    frame.width = (uint32_t)width;
    frame.height = (uint32_t)height;
    frame.bytes = [NSData dataWithBytes:pixels length:count * 4];
    free(pixels);
    [self.frames addObject:frame];
}

- (uint64_t)openWithWidth:(uint32_t)width height:(uint32_t)height
{
    ensure_app();

    if (width == 0 || height == 0)
    {
        return 0;
    }

    EngineWebSlot *slot = [EngineWebSlot new];
    slot.host = self;
    slot.viewId = self.nextId;
    self.nextId += 1;
    slot.width = width;
    slot.height = height;

    NSRect frame = NSMakeRect(-16000, -16000, width, height);
    NSWindow *window = [[NSWindow alloc] initWithContentRect:frame
                                                    styleMask:NSWindowStyleMaskBorderless
                                                      backing:NSBackingStoreBuffered
                                                        defer:NO];
    window.releasedWhenClosed = NO;
    window.opaque = NO;
    window.backgroundColor = [NSColor clearColor];
    window.hasShadow = NO;
    window.ignoresMouseEvents = YES;
    window.hidesOnDeactivate = NO;
    window.collectionBehavior = NSWindowCollectionBehaviorStationary | NSWindowCollectionBehaviorIgnoresCycle | NSWindowCollectionBehaviorTransient;
    window.level = NSNormalWindowLevel;
    [window setAcceptsMouseMovedEvents:YES];

    WKWebViewConfiguration *config = [WKWebViewConfiguration new];
    WKUserContentController *users = [WKUserContentController new];
    WKUserScript *script = [[WKUserScript alloc] initWithSource:bridge_script()
                                                  injectionTime:WKUserScriptInjectionTimeAtDocumentStart
                                               forMainFrameOnly:YES];
    [users addUserScript:script];
    [users addScriptMessageHandler:slot name:@"engine"];
    config.userContentController = users;

    WKWebView *web = [[WKWebView alloc] initWithFrame:NSMakeRect(0, 0, width, height) configuration:config];
    web.navigationDelegate = slot;

    if ([web respondsToSelector:@selector(setUnderPageBackgroundColor:)])
    {
        web.underPageBackgroundColor = [NSColor clearColor];
    }

    [window.contentView addSubview:web];
    [window orderFrontRegardless];
    [window setFrameOrigin:NSMakePoint(-16000, -16000)];
    slot.window = window;
    slot.web = web;
    [self.slots addObject:slot];

    return slot.viewId;
}

- (void)closeView:(uint64_t)viewId
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil)
    {
        return;
    }

    [slot.web stopLoading];
    [slot.web.configuration.userContentController removeScriptMessageHandlerForName:@"engine"];
    slot.web.navigationDelegate = nil;
    [slot.window orderOut:nil];
    [slot.window close];
    [self.slots removeObject:slot];
}

- (void)loadHTML:(uint64_t)viewId html:(const char *)html
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil)
    {
        return;
    }

    slot.live = YES;
    slot.boost = YES;
    [slot.web loadHTMLString:ns_text(html) baseURL:nil];
}

- (void)loadURL:(uint64_t)viewId url:(const char *)url
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil)
    {
        return;
    }

    NSURL *parsed = [NSURL URLWithString:ns_text(url)];

    if (parsed == nil)
    {
        return;
    }

    slot.live = YES;
    slot.boost = YES;
    [slot.web loadRequest:[NSURLRequest requestWithURL:parsed]];
}

- (void)resize:(uint64_t)viewId width:(uint32_t)width height:(uint32_t)height
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil || width == 0 || height == 0)
    {
        return;
    }

    slot.width = width;
    slot.height = height;
    [slot.window setContentSize:NSMakeSize(width, height)];
    slot.web.frame = NSMakeRect(0, 0, width, height);
    [slot.window setFrameOrigin:NSMakePoint(-16000, -16000)];
    slot.boost = YES;
}

- (void)runJS:(uint64_t)viewId code:(const char *)code
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil)
    {
        return;
    }

    [slot.web evaluateJavaScript:ns_text(code) completionHandler:nil];
    slot.boost = YES;
}

- (void)mouseMove:(uint64_t)viewId x:(float)x y:(float)y
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil)
    {
        return;
    }

    slot.lastX = x;
    slot.lastY = y;
    NSPoint where = [slot pointForX:x y:y];
    NSEventType type = NSEventTypeMouseMoved;

    if ((slot.buttons & 1) != 0)
    {
        type = NSEventTypeLeftMouseDragged;
    }
    else if ((slot.buttons & 2) != 0)
    {
        type = NSEventTypeRightMouseDragged;
    }
    else if ((slot.buttons & 4) != 0)
    {
        type = NSEventTypeOtherMouseDragged;
    }

    NSEvent *event = [slot mouseEventOfType:type at:where];

    if (event == nil)
    {
        return;
    }

    if (type == NSEventTypeLeftMouseDragged)
    {
        [slot.web mouseDragged:event];
    }
    else if (type == NSEventTypeRightMouseDragged)
    {
        [slot.web rightMouseDragged:event];
    }
    else if (type == NSEventTypeOtherMouseDragged)
    {
        [slot.web otherMouseDragged:event];
    }
    else
    {
        [slot.web mouseMoved:event];
    }
}

- (void)mouseButton:(uint64_t)viewId button:(int)button down:(int)down
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil)
    {
        return;
    }

    int bit = 1;

    if (button == 2)
    {
        bit = 2;
    }
    else if (button == 3)
    {
        bit = 4;
    }

    if (down)
    {
        slot.buttons |= bit;
    }
    else
    {
        slot.buttons &= ~bit;
    }

    NSPoint where = [slot pointForX:slot.lastX y:slot.lastY];
    NSEventType type = NSEventTypeLeftMouseDown;

    if (button == 2)
    {
        type = down ? NSEventTypeRightMouseDown : NSEventTypeRightMouseUp;
    }
    else if (button == 3)
    {
        type = down ? NSEventTypeOtherMouseDown : NSEventTypeOtherMouseUp;
    }
    else
    {
        type = down ? NSEventTypeLeftMouseDown : NSEventTypeLeftMouseUp;
    }

    NSEvent *event = [slot mouseEventOfType:type at:where];

    if (event == nil)
    {
        return;
    }

    if (button == 2)
    {
        if (down)
        {
            [slot.web rightMouseDown:event];
        }
        else
        {
            [slot.web rightMouseUp:event];
        }
    }
    else if (button == 3)
    {
        if (down)
        {
            [slot.web otherMouseDown:event];
        }
        else
        {
            [slot.web otherMouseUp:event];
        }
    }
    else if (down)
    {
        [slot.web mouseDown:event];
    }
    else
    {
        [slot.web mouseUp:event];
    }
}

- (void)mouseWheel:(uint64_t)viewId dx:(float)dx dy:(float)dy
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil)
    {
        return;
    }

    NSString *script = [NSString stringWithFormat:@"(function(){var node=document.elementFromPoint(%f,%f)||document.scrollingElement||document.body;if(!node){return;}node.dispatchEvent(new WheelEvent('wheel',{deltaX:%f,deltaY:%f,bubbles:true,cancelable:true}));})()", slot.lastX, slot.lastY, (double)dx, (double)-dy];
    [slot.web evaluateJavaScript:script completionHandler:nil];
}

- (void)key:(uint64_t)viewId
       code:(unsigned short)code
      chars:(const char *)chars
       down:(int)down
     repeat:(int)repeat
       mods:(uint32_t)mods
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil)
    {
        return;
    }

    NSString *text = ns_text(chars);

    if (text.length == 0)
    {
        text = @" ";
    }

    NSEvent *event = [NSEvent keyEventWithType:(down ? NSEventTypeKeyDown : NSEventTypeKeyUp)
                                       location:NSZeroPoint
                                  modifierFlags:(NSEventModifierFlags)mods
                                      timestamp:[NSProcessInfo processInfo].systemUptime
                                   windowNumber:slot.window.windowNumber
                                        context:nil
                                     characters:text
                    charactersIgnoringModifiers:text
                                      isARepeat:repeat ? YES : NO
                                        keyCode:code];

    if (event == nil)
    {
        return;
    }

    if (down)
    {
        [slot.web keyDown:event];
    }
    else
    {
        [slot.web keyUp:event];
    }
}

- (void)insert:(uint64_t)viewId text:(const char *)text
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil || text == NULL || text[0] == 0)
    {
        return;
    }

    [slot.web insertText:ns_text(text)];
}

- (void)focus:(uint64_t)viewId on:(int)on
{
    EngineWebSlot *slot = [self slotFor:viewId];

    if (slot == nil)
    {
        return;
    }

    if (on)
    {
        [slot.window makeFirstResponder:slot.web];
    }
}

- (void)shutdown
{
    NSArray<EngineWebSlot *> *copy = [self.slots copy];
    NSUInteger idx = 0;

    while (idx < copy.count)
    {
        [self closeView:copy[idx].viewId];
        idx += 1;
    }
}

@end

static EngineWebHost *host_of(void *host)
{
    return (__bridge EngineWebHost *)host;
}

void *engine_web_new(void)
{
    EngineWebHost *host = [EngineWebHost new];

    return (void *)CFBridgingRetain(host);
}

void engine_web_free(void *host)
{
    if (host == NULL)
    {
        return;
    }

    EngineWebHost *obj = (EngineWebHost *)CFBridgingRelease(host);
    [obj shutdown];
}

uint64_t engine_web_create(void *host, uint32_t width, uint32_t height)
{
    EngineWebHost *obj = host_of(host);

    if (obj == nil)
    {
        return 0;
    }

    return [obj openWithWidth:width height:height];
}

void engine_web_destroy(void *host, uint64_t view_id)
{
    [host_of(host) closeView:view_id];
}

void engine_web_load_html(void *host, uint64_t view_id, const char *html)
{
    [host_of(host) loadHTML:view_id html:html];
}

void engine_web_load_url(void *host, uint64_t view_id, const char *url)
{
    [host_of(host) loadURL:view_id url:url];
}

void engine_web_resize(void *host, uint64_t view_id, uint32_t width, uint32_t height)
{
    [host_of(host) resize:view_id width:width height:height];
}

void engine_web_run_js(void *host, uint64_t view_id, const char *code)
{
    [host_of(host) runJS:view_id code:code];
}

void engine_web_mouse_move(void *host, uint64_t view_id, float x, float y)
{
    [host_of(host) mouseMove:view_id x:x y:y];
}

void engine_web_mouse_button(void *host, uint64_t view_id, int button, int down)
{
    [host_of(host) mouseButton:view_id button:button down:down];
}

void engine_web_mouse_wheel(void *host, uint64_t view_id, float dx, float dy)
{
    [host_of(host) mouseWheel:view_id dx:dx dy:dy];
}

void engine_web_key(void *host, uint64_t view_id, unsigned short code, const char *chars, int down, int repeat, uint32_t mods)
{
    [host_of(host) key:view_id code:code chars:chars down:down repeat:repeat mods:mods];
}

void engine_web_text(void *host, uint64_t view_id, const char *text)
{
    [host_of(host) insert:view_id text:text];
}

void engine_web_focus(void *host, uint64_t view_id, int on)
{
    [host_of(host) focus:view_id on:on];
}

int engine_web_is_live(void *host, uint64_t view_id)
{
    EngineWebSlot *slot = [host_of(host) slotFor:view_id];

    if (slot == nil || !slot.live)
    {
        return 0;
    }

    return 1;
}

int engine_web_boost(void *host, uint64_t view_id)
{
    EngineWebSlot *slot = [host_of(host) slotFor:view_id];

    if (slot == nil || !slot.boost)
    {
        return 0;
    }

    return 1;
}

void engine_web_clear_boost(void *host, uint64_t view_id)
{
    EngineWebSlot *slot = [host_of(host) slotFor:view_id];

    if (slot != nil)
    {
        slot.boost = NO;
    }
}

int engine_web_busy(void *host, uint64_t view_id)
{
    EngineWebSlot *slot = [host_of(host) slotFor:view_id];

    if (slot == nil || !slot.busy)
    {
        return 0;
    }

    return 1;
}

void engine_web_request_snapshot(void *host, uint64_t view_id)
{
    EngineWebSlot *slot = [host_of(host) slotFor:view_id];

    if (slot != nil)
    {
        [slot requestSnapshot];
    }
}

int engine_web_poll_frame(void *host, uint64_t *view_id, uint32_t *width, uint32_t *height, uint8_t **bytes, uint32_t *length)
{
    EngineWebHost *obj = host_of(host);

    if (obj == nil || obj.frames.count == 0)
    {
        return 0;
    }

    EngineWebFrame *frame = obj.frames.firstObject;
    uint32_t size = (uint32_t)frame.bytes.length;
    uint8_t *copy = (uint8_t *)malloc(size == 0 ? 1 : size);

    if (copy == NULL)
    {
        return 0;
    }

    if (size > 0)
    {
        memcpy(copy, frame.bytes.bytes, size);
    }

    *view_id = frame.viewId;
    *width = frame.width;
    *height = frame.height;
    *bytes = copy;
    *length = size;
    [obj.frames removeObjectAtIndex:0];

    return 1;
}

void engine_web_free_bytes(uint8_t *bytes)
{
    free(bytes);
}

int engine_web_poll_message(void *host, uint64_t *view_id, char **text)
{
    EngineWebHost *obj = host_of(host);

    if (obj == nil || obj.notes.count == 0)
    {
        return 0;
    }

    EngineWebNote *note = obj.notes.firstObject;
    const char *utf = note.text.UTF8String;

    if (utf == NULL)
    {
        utf = "";
    }

    size_t len = strlen(utf);
    char *copy = (char *)malloc(len + 1);

    if (copy == NULL)
    {
        return 0;
    }

    memcpy(copy, utf, len + 1);
    *view_id = note.viewId;
    *text = copy;
    [obj.notes removeObjectAtIndex:0];

    return 1;
}

void engine_web_free_text(char *text)
{
    free(text);
}

void engine_web_debug_message(void *host, uint64_t view_id, const char *text)
{
    [host_of(host) pushMessage:view_id text:ns_text(text)];
}

void engine_web_pump_runloop(double seconds)
{
    ensure_app();
    NSDate *until = [NSDate dateWithTimeIntervalSinceNow:seconds];
    [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:until];
}
