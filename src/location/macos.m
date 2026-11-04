#import <Foundation/Foundation.h>
#import <CoreLocation/CoreLocation.h>

@interface LocationDelegate : NSObject <CLLocationManagerDelegate>
@property BOOL done;
@property BOOL started;
@property int result;
@end
@implementation LocationDelegate
- (void)fail:(NSString *)message {
    fprintf(stderr, "%s\n", message.UTF8String);
    self.result = 1;
    self.done = YES;
}
- (void)authorize:(CLLocationManager *)manager {
    CLAuthorizationStatus status = manager.authorizationStatus;
    if (status == kCLAuthorizationStatusDenied || status == kCLAuthorizationStatusRestricted) {
        [self fail:@"Location permission denied. Enable Location Services in System Settings > Privacy & Security."];
    } else if (status != kCLAuthorizationStatusNotDetermined && !self.started) {
        self.started = YES;
        [manager startUpdatingLocation];
    }
}
- (void)locationManagerDidChangeAuthorization:(CLLocationManager *)manager { [self authorize:manager]; }
- (void)locationManager:(CLLocationManager *)manager didChangeAuthorizationStatus:(CLAuthorizationStatus)status { [self authorize:manager]; }
- (void)locationManager:(CLLocationManager *)manager didUpdateLocations:(NSArray<CLLocation *> *)locations {
    CLLocation *location = locations.lastObject;
    if (location.horizontalAccuracy < 0 || fabs(location.timestamp.timeIntervalSinceNow) > 60) return;
    NSDictionary *data = @{ @"latitude": @(location.coordinate.latitude), @"longitude": @(location.coordinate.longitude),
                           @"accuracy": @(location.horizontalAccuracy), @"city": [NSNull null] };
    NSData *json = [NSJSONSerialization dataWithJSONObject:data options:0 error:nil];
    fwrite(json.bytes, 1, json.length, stdout);
    self.done = YES;
}
- (void)locationManager:(CLLocationManager *)manager didFailWithError:(NSError *)error {
    if (error.code != kCLErrorLocationUnknown) [self fail:error.localizedDescription];
}
@end

int main(void) {
    @autoreleasepool {
        if (![CLLocationManager locationServicesEnabled]) {
            fprintf(stderr, "Location Services are disabled.\n"); return 1;
        }
        LocationDelegate *delegate = [LocationDelegate new];
        CLLocationManager *manager = [CLLocationManager new];
        manager.delegate = delegate;
        manager.desiredAccuracy = kCLLocationAccuracyBest;
        [manager requestWhenInUseAuthorization];
        [delegate authorize:manager];
        NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:55];
        while (!delegate.done && deadline.timeIntervalSinceNow > 0) {
            [[NSRunLoop currentRunLoop] runUntilDate:[NSDate dateWithTimeIntervalSinceNow:0.1]];
        }
        [manager stopUpdatingLocation];
        if (!delegate.done) [delegate fail:@"Location timed out."];
        return delegate.result;
    }
}
