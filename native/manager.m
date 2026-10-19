#import <AppKit/AppKit.h>
#import <ServiceManagement/ServiceManagement.h>
extern char *mini_catalog(void);
extern char *mini_check(const char *,const char *,uint64_t);
extern char *mini_install(const char *,const char *,const char *,int (*)(const char *,const char *));
extern void mini_free(char *);
static NSString *const ManagedKey=@"managedByRustlet";
static NSString *const ManagedChanged=@"RustletManagementChanged";
static id Decode(char *json) {
    if (!json) return @{};
    NSData *data=[[NSString stringWithUTF8String:json] dataUsingEncoding:NSUTF8StringEncoding]; mini_free(json);
    return [NSJSONSerialization JSONObjectWithData:data options:0 error:nil] ?: @{};
}
static NSString *OSVersion(void) {
    NSOperatingSystemVersion v=NSProcessInfo.processInfo.operatingSystemVersion;
    return [NSString stringWithFormat:@"%ld.%ld.%ld",v.majorVersion,v.minorVersion,v.patchVersion];
}
static BOOL External(NSDictionary *tool) { return [tool[@"integration"] isEqual:@"external"]; }
static void Manage(NSDictionary *tool) {
    if (External(tool)) return;
    CFStringRef domain=(__bridge CFStringRef)tool[@"bundle_id"];
    CFPreferencesSetAppValue((__bridge CFStringRef)ManagedKey,kCFBooleanTrue,domain);
    CFPreferencesAppSynchronize(domain);
    [NSDistributedNotificationCenter.defaultCenter postNotificationName:ManagedChanged object:tool[@"bundle_id"]];
}
static NSDictionary *Info(NSURL *url) {
    return url ? [NSDictionary dictionaryWithContentsOfURL:[url URLByAppendingPathComponent:@"Contents/Info.plist"]] ?: @{} : @{};
}
static NSURL *Installed(NSDictionary *tool) {
    NSString *selected=[NSUserDefaults.standardUserDefaults stringForKey:[@"applicationPath." stringByAppendingString:tool[@"id"]]];
    if (selected.length) {
        NSURL *url=[NSURL fileURLWithPath:selected];
        if ([Info(url)[@"CFBundleIdentifier"] isEqual:tool[@"bundle_id"]]) return url;
    }
    for (NSString *root in @[@"/Applications",[NSHomeDirectory() stringByAppendingPathComponent:@"Applications"]]) {
        NSURL *url=[NSURL fileURLWithPath:[root stringByAppendingPathComponent:tool[@"bundle_name"]]];
        if ([Info(url)[@"CFBundleIdentifier"] isEqual:tool[@"bundle_id"]]) return url;
    }
    return nil;
}
static NSArray<NSRunningApplication *> *Running(NSDictionary *tool,NSURL *url) {
    if (!url) return @[];
    NSMutableArray *apps=[NSMutableArray new];
    for (NSRunningApplication *app in [NSRunningApplication runningApplicationsWithBundleIdentifier:tool[@"bundle_id"]])
        if ([app.bundleURL.URLByResolvingSymlinksInPath.path isEqual:url.URLByResolvingSymlinksInPath.path] && !app.terminated) [apps addObject:app];
    return apps;
}
// Called by Rust on a background queue after signature/notarization checks.
static int StopForInstall(const char *identifier,const char *path) {
    NSString *toolID=[NSString stringWithUTF8String:identifier];
    NSURL *url=[NSURL fileURLWithPath:[NSString stringWithUTF8String:path]];
    __block NSArray *apps=@[];
    dispatch_sync(dispatch_get_main_queue(), ^{
        NSArray *catalog=Decode(mini_catalog());
        for (NSDictionary *tool in catalog) if ([tool[@"id"] isEqual:toolID]) { apps=Running(tool,url); break; }
        for (NSRunningApplication *app in apps) [app terminate];
    });
    if (!apps.count) return 0;
    double deadline=NSProcessInfo.processInfo.systemUptime+12;
    while (NSProcessInfo.processInfo.systemUptime<deadline) {
        __block BOOL done=YES;
        dispatch_sync(dispatch_get_main_queue(), ^{ for (NSRunningApplication *app in apps) if (!app.terminated) done=NO; });
        if (done) return 1;
        [NSThread sleepForTimeInterval:0.05];
    }
    return -1;
}
@interface Rustlet : NSObject <NSApplicationDelegate>
@property(nonatomic,strong) NSStatusItem *status;
@property(nonatomic,strong) NSWindow *window;
@property(nonatomic,strong) NSArray<NSDictionary *> *tools;
@property(nonatomic,strong) NSMutableDictionary<NSString *,NSDictionary *> *rows;
@property(nonatomic,strong) NSMutableDictionary<NSString *,NSDictionary *> *releases;
@property(nonatomic,strong) NSMutableSet<NSString *> *busy,*checking;
@property(nonatomic,strong) NSTimer *timer,*checkTimer;
@property(nonatomic,strong) NSButton *checkButton,*automatic,*login;
@end
@implementation Rustlet
- (NSTextField *)label:(NSString *)text frame:(NSRect)frame in:(NSView *)view {
    NSTextField *label=[NSTextField wrappingLabelWithString:text]; label.frame=frame; [view addSubview:label]; return label;
}
- (BOOL)automaticChecks { return ![NSUserDefaults.standardUserDefaults objectForKey:@"automaticChecks"] || [NSUserDefaults.standardUserDefaults boolForKey:@"automaticChecks"]; }
- (NSDictionary *)tool:(NSString *)identifier { for (NSDictionary *tool in self.tools) if ([tool[@"id"] isEqual:identifier]) return tool; return nil; }
- (void)applicationDidFinishLaunching:(NSNotification *)note {
    (void)note; self.tools=(NSArray *)Decode(mini_catalog()); self.rows=[NSMutableDictionary new];
    self.releases=[NSMutableDictionary new]; self.busy=[NSMutableSet new]; self.checking=[NSMutableSet new];
    self.status=[NSStatusBar.systemStatusBar statusItemWithLength:NSSquareStatusItemLength];
    self.status.autosaveName=@"RustletStatusItem"; self.status.visible=YES;
    if (![NSUserDefaults.standardUserDefaults boolForKey:@"compactMenuPositionV1"]) {
        [NSUserDefaults.standardUserDefaults setDouble:0 forKey:@"NSStatusItem Preferred Position RustletStatusItem"];
        [NSUserDefaults.standardUserDefaults setBool:YES forKey:@"compactMenuPositionV1"];
    }
    NSImage *icon=[NSImage imageWithSystemSymbolName:@"square.stack.3d.up" accessibilityDescription:@"Rustlet"];
    icon.size=NSMakeSize(18,18); icon.template=YES; self.status.button.image=icon;
    self.status.button.accessibilityLabel=@"Rustlet · správce nástrojů"; self.status.button.toolTip=@"Rustlet · správce nástrojů";
    [self makeWindow]; [self refresh];
    for (NSDictionary *tool in self.tools) if (Installed(tool)) {
        Manage(tool);
        NSString *enabledKey=[@"enabled." stringByAppendingString:tool[@"id"]];
        if (![NSUserDefaults.standardUserDefaults objectForKey:enabledKey]) [self remember:tool enabled:Running(tool,Installed(tool)).count>0];
        if ([NSUserDefaults.standardUserDefaults boolForKey:[@"enabled." stringByAppendingString:tool[@"id"]]]) [self openTool:tool settings:NO];
    }
    self.timer=[NSTimer scheduledTimerWithTimeInterval:1 target:self selector:@selector(refresh) userInfo:nil repeats:YES];
    [NSRunLoop.mainRunLoop addTimer:self.timer forMode:NSRunLoopCommonModes];
    self.checkTimer=[NSTimer scheduledTimerWithTimeInterval:3600 target:self selector:@selector(scheduledCheck:) userInfo:nil repeats:YES];
    [NSDistributedNotificationCenter.defaultCenter addObserver:self selector:@selector(checkRequested:) name:@"RustletCheckUpdates" object:nil];
    if ([self automaticChecks]) [self checkAll:nil];
    if (![NSProcessInfo.processInfo.arguments containsObject:@"--background"]) [self show:nil];
}
- (void)makeWindow {
    self.window=[[NSWindow alloc] initWithContentRect:NSMakeRect(0,0,660,520) styleMask:NSWindowStyleMaskTitled|NSWindowStyleMaskClosable|NSWindowStyleMaskMiniaturizable backing:NSBackingStoreBuffered defer:NO];
    self.window.title=@"Rustlet · Nástroje"; self.window.releasedWhenClosed=NO; [self.window center];
    NSView *view=self.window.contentView;
    [self label:@"Malé aplikace. Jedno místo." frame:NSMakeRect(28,445,600,36) in:view].font=[NSFont systemFontOfSize:24 weight:NSFontWeightSemibold];
    [self label:@"Tvoje nástroje pro Mac, připravené k zapnutí." frame:NSMakeRect(28,410,600,24) in:view].textColor=NSColor.secondaryLabelColor;
    NSScrollView *scroll=[[NSScrollView alloc] initWithFrame:NSMakeRect(20,132,620,260)];
    scroll.hasVerticalScroller=YES; scroll.drawsBackground=NO;
    CGFloat height=MAX(260,self.tools.count*230);
    NSView *list=[[NSView alloc] initWithFrame:NSMakeRect(0,0,600,height)]; scroll.documentView=list; [view addSubview:scroll];
    NSUInteger index=0;
    for (NSDictionary *tool in self.tools) {
        CGFloat y=height-(++index)*230;
        NSBox *row=[[NSBox alloc] initWithFrame:NSMakeRect(4,y+8,588,212)]; row.boxType=NSBoxCustom; row.cornerRadius=10;
        row.borderColor=NSColor.separatorColor; row.fillColor=NSColor.controlBackgroundColor; [list addSubview:row];
        NSView *body=row.contentView;
        NSImageView *icon=[[NSImageView alloc] initWithFrame:NSMakeRect(20,145,34,34)];
        icon.image=[NSImage imageWithSystemSymbolName:tool[@"symbol"] accessibilityDescription:nil]; [body addSubview:icon];
        [self label:tool[@"name"] frame:NSMakeRect(68,155,330,26) in:body].font=[NSFont systemFontOfSize:17 weight:NSFontWeightSemibold];
        NSTextField *state=[self label:@"" frame:NSMakeRect(420,157,130,24) in:body]; state.alignment=NSTextAlignmentRight;
        [self label:tool[@"description"] frame:NSMakeRect(20,106,530,40) in:body].textColor=NSColor.secondaryLabelColor;
        NSTextField *version=[self label:@"" frame:NSMakeRect(20,78,530,24) in:body]; version.font=[NSFont monospacedDigitSystemFontOfSize:12 weight:NSFontWeightRegular];
        NSButton *toggle=[NSButton checkboxWithTitle:@"Zapnuto" target:self action:@selector(toggleTool:)]; toggle.frame=NSMakeRect(20,22,116,32); toggle.identifier=tool[@"id"]; [body addSubview:toggle];
        NSButton *settings=[NSButton buttonWithTitle:External(tool) ? @"Otevřít…" : @"Nastavení…" target:self action:@selector(settingsTool:)]; settings.frame=NSMakeRect(146,22,130,32); settings.identifier=tool[@"id"]; [body addSubview:settings];
        NSButton *install=[NSButton buttonWithTitle:@"Nainstalovat" target:self action:@selector(installTool:)]; install.frame=NSMakeRect(380,22,174,32); install.identifier=tool[@"id"]; [body addSubview:install];
        NSTextField *message=[self label:External(tool) ? @"Aktualizace spravuje samotná aplikace." : @"" frame:NSMakeRect(20,52,534,24) in:body]; message.font=[NSFont systemFontOfSize:11]; message.textColor=NSColor.secondaryLabelColor; message.maximumNumberOfLines=1;
        self.rows[tool[@"id"]]=@{@"state":state,@"version":version,@"toggle":toggle,@"settings":settings,@"install":install,@"message":message};
    }
    [scroll.contentView scrollToPoint:NSMakePoint(0,MAX(0,height-scroll.contentSize.height))];
    [scroll reflectScrolledClipView:scroll.contentView];
    self.checkButton=[NSButton buttonWithTitle:@"Zkontrolovat verze" target:self action:@selector(checkAll:)];
    self.checkButton.frame=NSMakeRect(28,86,194,32); [view addSubview:self.checkButton];
    self.automatic=[NSButton checkboxWithTitle:@"Hledat aktualizace každou hodinu" target:self action:@selector(changeAutomatic:)];
    self.automatic.frame=NSMakeRect(242,86,390,32); [view addSubview:self.automatic];
    self.login=[NSButton checkboxWithTitle:@"Spustit Rustlet při přihlášení" target:self action:@selector(changeLogin:)]; self.login.frame=NSMakeRect(28,40,370,30); [view addSubview:self.login];
}
- (void)show:(id)sender { (void)sender; [NSApp activateIgnoringOtherApps:YES]; [self.window makeKeyAndOrderFront:nil]; }
- (void)refresh {
    NSMenu *menu=[NSMenu new]; [self item:@"Rustlet" action:nil tool:nil menu:menu]; [menu addItem:NSMenuItem.separatorItem];
    for (NSDictionary *tool in self.tools) {
        NSString *identifier=tool[@"id"]; NSDictionary *row=self.rows[identifier];
        NSURL *url=Installed(tool); BOOL running=Running(tool,url).count>0,busy=[self.busy containsObject:identifier];
        NSDictionary *info=Info(url);
        NSDictionary *release=self.releases[identifier]; uint64_t build=[info[@"CFBundleVersion"] longLongValue];
        BOOL available=[release[@"ok"] boolValue] && [release[@"build"] unsignedLongLongValue]>build;
        NSTextField *state=row[@"state"]; state.stringValue=running ? @"Běží" : url ? @"Vypnuto" : @"Nenainstalováno";
        state.textColor=running ? NSColor.systemGreenColor : NSColor.secondaryLabelColor;
        NSTextField *version=row[@"version"]; version.stringValue=url ? [NSString stringWithFormat:@"Nainstalováno %@%@",info[@"CFBundleShortVersionString"] ?: @"?",available ? [@"  ·  Nová verze " stringByAppendingString:release[@"version"]] : @""] : @"Stáhni nástroj do Applications.";
        NSButton *toggle=row[@"toggle"]; toggle.state=running ? NSControlStateValueOn : NSControlStateValueOff; toggle.enabled=url && !busy;
        [row[@"settings"] setEnabled:url && !busy];
        NSButton *install=row[@"install"]; install.title=busy ? @"Pracuji…" : !url ? @"Nainstalovat" : ![release[@"ok"] boolValue] ? @"Zkontrolovat" : available ? @"Aktualizovat" : @"Aktuální verze";
        install.enabled=!busy && (!url || available || ![release[@"ok"] boolValue]);
        if (External(tool)) {
            install.title=busy ? @"Pracuji…" : url ? @"Přidat aplikaci…" : @"Nainstalovat"; install.enabled=!busy;
        }
        NSString *title=[NSString stringWithFormat:@"%@ · %@",tool[@"name"],running ? @"Běží" : url ? @"Vypnuto" : @"Nenainstalováno"];
        NSMenuItem *parent=[self item:title action:nil tool:nil menu:menu]; NSMenu *sub=[NSMenu new]; parent.submenu=sub; parent.enabled=YES;
        NSMenuItem *power=[self item:running ? @"Vypnout" : @"Zapnout" action:@selector(toggleTool:) tool:identifier menu:sub]; power.enabled=url && !busy;
        NSMenuItem *settings=[self item:External(tool) ? @"Otevřít aplikaci…" : @"Nastavení nástroje…" action:@selector(settingsTool:) tool:identifier menu:sub]; settings.enabled=url && !busy;
        NSMenuItem *update=[self item:External(tool) && url ? @"Přidat aplikaci…" : !url ? @"Nainstalovat…" : @"Aktualizovat…" action:@selector(installTool:) tool:identifier menu:sub]; update.enabled=!busy && (External(tool) || !url || available || ![release[@"ok"] boolValue]);
        if (External(tool) && !url) {
            NSMenuItem *choose=[self item:@"Vybrat existující aplikaci…" action:@selector(chooseApplication:) tool:identifier menu:sub]; choose.enabled=!busy;
        }
    }
    [menu addItem:NSMenuItem.separatorItem];
    [self item:@"Otevřít správce…" action:@selector(show:) tool:nil menu:menu];
    [self item:@"Zkontrolovat verze" action:@selector(checkAll:) tool:nil menu:menu];
    [menu addItem:NSMenuItem.separatorItem]; [self item:@"Ukončit Rustlet" action:@selector(quit:) tool:nil menu:menu];
    menu.autoenablesItems=NO; for (NSMenuItem *item in menu.itemArray) item.submenu.autoenablesItems=NO;
    self.status.menu=menu;
    self.checkButton.enabled=self.busy.count==0 && self.checking.count==0; self.automatic.state=[self automaticChecks] ? NSControlStateValueOn : NSControlStateValueOff;
    if (@available(macOS 13.0,*)) { self.login.state=SMAppService.mainAppService.status==SMAppServiceStatusEnabled ? NSControlStateValueOn : NSControlStateValueOff; }
    else self.login.enabled=NO;
}
- (NSMenuItem *)item:(NSString *)title action:(SEL)action tool:(NSString *)identifier menu:(NSMenu *)menu {
    NSMenuItem *item=[[NSMenuItem alloc] initWithTitle:title action:action keyEquivalent:@""]; item.target=self; item.representedObject=identifier;
    item.enabled=action!=nil; [menu addItem:item]; return item;
}
- (NSString *)identifier:(id)sender { return [sender isKindOfClass:NSMenuItem.class] ? [sender representedObject] : [sender identifier]; }
- (void)remember:(NSDictionary *)tool enabled:(BOOL)enabled {
    NSString *key=[@"enabled." stringByAppendingString:tool[@"id"]];
    [NSUserDefaults.standardUserDefaults setBool:enabled forKey:key];
    CFPreferencesSetAppValue((__bridge CFStringRef)key,enabled ? kCFBooleanTrue : kCFBooleanFalse,CFSTR("cz.attosoft.tools-manager"));
    CFPreferencesAppSynchronize(CFSTR("cz.attosoft.tools-manager"));
}
- (void)openTool:(NSDictionary *)tool settings:(BOOL)settings {
    NSURL *url=Installed(tool); if (!url) return; Manage(tool);
    BOOL previous=[NSUserDefaults.standardUserDefaults boolForKey:[@"enabled." stringByAppendingString:tool[@"id"]]];
    [self remember:tool enabled:YES];
    NSWorkspaceOpenConfiguration *config=[NSWorkspaceOpenConfiguration configuration]; config.activates=settings || External(tool);
    config.arguments=External(tool) ? @[] : settings ? @[@"--settings"] : @[@"--background"];
    [NSWorkspace.sharedWorkspace openApplicationAtURL:url configuration:config completionHandler:^(NSRunningApplication *app,NSError *error) {
        dispatch_async(dispatch_get_main_queue(), ^{
            if (error) { [self remember:tool enabled:previous]; [self message:error.localizedDescription tool:tool]; }
            else if (app) [self remember:tool enabled:YES];
            [self refresh];
        });
    }];
}
- (void)toggleTool:(id)sender {
    NSDictionary *tool=[self tool:[self identifier:sender]]; if (!tool || [self.busy containsObject:tool[@"id"]]) return;
    NSArray *apps=Running(tool,Installed(tool));
    if (apps.count) {
        for (NSRunningApplication *app in apps) [app terminate];
        [self remember:tool enabled:NO];
        dispatch_after(dispatch_time(DISPATCH_TIME_NOW,2*NSEC_PER_SEC),dispatch_get_main_queue(), ^{
            for (NSRunningApplication *app in apps) if (!app.terminated) {
                [self message:@"Nástroj právě dokončuje práci. Zkus vypnutí znovu za chvíli." tool:tool]; break;
            }
        });
    }
    else [self openTool:tool settings:NO];
    [self refresh];
}
- (void)settingsTool:(id)sender { NSDictionary *tool=[self tool:[self identifier:sender]]; if (tool) [self openTool:tool settings:YES]; }
- (void)message:(NSString *)message tool:(NSDictionary *)tool {
    NSTextField *field=self.rows[tool[@"id"]][@"message"]; field.stringValue=message ?: @""; field.toolTip=message;
}
- (void)checkAll:(id)sender {
    (void)sender;
    for (NSDictionary *tool in self.tools) {
        if (External(tool)) continue;
        NSString *identifier=tool[@"id"]; if ([self.busy containsObject:identifier] || [self.checking containsObject:identifier]) continue;
        [self.checking addObject:identifier]; [self message:@"Hledám nové vydání…" tool:tool];
        NSURL *url=Installed(tool); uint64_t build=[Info(url)[@"CFBundleVersion"] longLongValue];
        NSString *os=OSVersion();
        dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY,0), ^{
            NSDictionary *result=Decode(mini_check(identifier.UTF8String,os.UTF8String,build));
            dispatch_async(dispatch_get_main_queue(), ^{
                self.releases[identifier]=result; [self.checking removeObject:identifier];
                [self message:[result[@"ok"] boolValue] ? ([result[@"available"] boolValue] ? @"Nová verze je připravená ke stažení." : @"Máš nejnovější dostupnou verzi.") : result[@"error"] tool:tool];
                [self refresh];
            });
        });
    }
    [self refresh];
}
- (void)chooseApplication:(id)sender {
    NSDictionary *tool=[self tool:[self identifier:sender]];
    if (tool && ![self.busy containsObject:tool[@"id"]]) { [self show:nil]; [self addApplication:tool]; }
}
- (void)addApplication:(NSDictionary *)tool {
    NSOpenPanel *panel=[NSOpenPanel openPanel];
    panel.title=[NSString stringWithFormat:@"Přidat %@ do správy",tool[@"name"]];
    panel.prompt=@"Přidat"; panel.canChooseFiles=YES; panel.canChooseDirectories=NO;
    panel.allowsMultipleSelection=NO; panel.treatsFilePackagesAsDirectories=NO;
    panel.directoryURL=[NSURL fileURLWithPath:@"/Applications"];
    [panel beginSheetModalForWindow:self.window completionHandler:^(NSModalResponse response) {
        if (response!=NSModalResponseOK) return;
        NSURL *url=panel.URL.URLByResolvingSymlinksInPath;
        if (![url.pathExtension.lowercaseString isEqual:@"app"] || ![Info(url)[@"CFBundleIdentifier"] isEqual:tool[@"bundle_id"]]) {
            NSAlert *alert=[NSAlert new]; alert.messageText=@"Jiná aplikace";
            alert.informativeText=[NSString stringWithFormat:@"Vyber aplikaci %@.",tool[@"name"]];
            [alert beginSheetModalForWindow:self.window completionHandler:nil]; return;
        }
        [NSUserDefaults.standardUserDefaults setObject:url.path forKey:[@"applicationPath." stringByAppendingString:tool[@"id"]]];
        [self remember:tool enabled:Running(tool,url).count>0];
        [self refresh];
    }];
}
- (void)installTool:(id)sender {
    NSDictionary *tool=[self tool:[self identifier:sender]]; if (!tool || [self.busy containsObject:tool[@"id"]]) return;
    if (External(tool) && Installed(tool)) { [self chooseApplication:sender]; return; }
    NSString *identifier=tool[@"id"];
    if (!External(tool) && Installed(tool) && ![self.releases[identifier][@"ok"] boolValue]) { [self checkAll:nil]; return; }
    [self.busy addObject:identifier]; [self message:@"Stahuji a ověřuji balíček…" tool:tool]; [self refresh];
    NSURL *target=Installed(tool);
    if (!target) {
        NSString *root=[NSFileManager.defaultManager isWritableFileAtPath:@"/Applications"] ? @"/Applications" : [NSHomeDirectory() stringByAppendingPathComponent:@"Applications"];
        target=[NSURL fileURLWithPath:[root stringByAppendingPathComponent:tool[@"bundle_name"]]];
    }
    NSString *path=target.path,*os=OSVersion();
    dispatch_async(dispatch_get_global_queue(QOS_CLASS_UTILITY,0), ^{
        NSDictionary *result=Decode(mini_install(identifier.UTF8String,path.UTF8String,os.UTF8String,StopForInstall));
        dispatch_async(dispatch_get_main_queue(), ^{
            [self.busy removeObject:identifier];
            if ([result[@"ok"] boolValue]) { Manage(tool); [self message:@"Nainstalováno." tool:tool]; }
            else [self message:result[@"error"] tool:tool];
            if ([result[@"relaunch"] boolValue] || (External(tool) && [result[@"ok"] boolValue])) [self openTool:tool settings:NO];
            [self refresh];
        });
    });
}
- (void)scheduledCheck:(NSTimer *)timer { (void)timer; if ([self automaticChecks]) [self checkAll:nil]; }
- (void)checkRequested:(NSNotification *)note { (void)note; [self show:nil]; [self checkAll:nil]; }
- (void)changeAutomatic:(NSButton *)sender { [NSUserDefaults.standardUserDefaults setBool:sender.state==NSControlStateValueOn forKey:@"automaticChecks"]; }
- (void)changeLogin:(NSButton *)sender {
    if (@available(macOS 13.0,*)) {
        NSError *error=nil; BOOL ok=sender.state==NSControlStateValueOn ? [SMAppService.mainAppService registerAndReturnError:&error] : [SMAppService.mainAppService unregisterAndReturnError:&error];
        if (!ok) { NSAlert *alert=[NSAlert new]; alert.messageText=@"Spuštění při přihlášení"; alert.informativeText=error.localizedDescription ?: @"Povol Rustlet v nastavení macOS."; [alert runModal]; }
    }
    [self refresh];
}
- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender { (void)sender; return self.busy.count ? NSTerminateCancel : NSTerminateNow; }
- (void)quit:(id)sender { (void)sender; [NSApp terminate:nil]; }
- (BOOL)applicationShouldHandleReopen:(NSApplication *)app hasVisibleWindows:(BOOL)visible { (void)app; (void)visible; [self show:nil]; return NO; }
- (void)applicationWillTerminate:(NSNotification *)note { (void)note; [self.timer invalidate]; [self.checkTimer invalidate]; [NSDistributedNotificationCenter.defaultCenter removeObserver:self]; }
@end
void rustlet_run(void) {
    @autoreleasepool {
        [NSApplication sharedApplication]; [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        Rustlet *delegate=[Rustlet new]; NSApp.delegate=delegate; [NSApp run];
    }
}
