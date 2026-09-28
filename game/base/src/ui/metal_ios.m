#import <UIKit/UIKit.h>
#import <QuartzCore/CAMetalLayer.h>

void engine_attach_metal_layer(void *view, void *layer, double width, double height)
{
    UIView *ui = (UIView *)view;
    CAMetalLayer *metal = (CAMetalLayer *)layer;
    metal.frame = CGRectMake(0.0, 0.0, width, height);
    metal.contentsScale = ui.contentScaleFactor;
    metal.opaque = YES;
    [ui.layer addSublayer:metal];
}

void engine_resize_metal_layer(void *layer, double width, double height)
{
    CAMetalLayer *metal = (CAMetalLayer *)layer;
    metal.frame = CGRectMake(0.0, 0.0, width, height);
}
