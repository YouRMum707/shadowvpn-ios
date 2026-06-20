#pragma once
#import <Foundation/Foundation.h>
#import <NetworkExtension/NetworkExtension.h>

// Builds the NEPacketTunnelNetworkSettings for the tunnel from the active
// Profile-derived config the provider passes in. ShadowVPN does all of its
// split-routing here in Swift/ObjC via includedRoutes/excludedRoutes — the Rust
// core only sees a raw bidirectional IP pipe; it does not know or care which IPs
// are tunneled. See DESIGN.md "Routing on iOS".
@interface SVTunnelSettings : NSObject

/// @param serverAddress  Tunnel remote address. iOS validates this and rejects a
///                       bare DNS hostname ("Invalid ... tunnelRemoteAddress"),
///                       so the provider resolves the server host to a dotted
///                       IPv4 literal before passing it here.
/// @param mode           "full" | "chnroute" | "chinadns".
/// @param dnsLocal       Domestic DNS upstream "host:port" (chinadns only).
/// @param dnsRemote      Clean DNS upstream "host:port" (chinadns only).
/// @param mtu            Tunnel MTU (Profile.mtu, default 1400).
/// @param chnrouteURL    File URL of chnroute.txt (the NE's own bundle copy or
///                       the App-Group staged copy). Read for chnroute/chinadns
///                       to append every China CIDR as an excluded route. May be
///                       nil for "full".
/// @param serverExclusions  Dotted-IPv4 addresses of the server itself. Each is
///                       added as a /32 excluded route so the core's encrypted
///                       UDP socket to the server goes out the physical
///                       interface instead of looping back into the tunnel
///                       (essential when the server is outside the bypass set,
///                       e.g. an overseas server in chnroute mode). May be nil.
/// @param tunnelIP  The inner client IPv4 address assigned to the TUN (must
///                   match the server's `peer_ip`). The `10.0.0.0/8` LAN bypass
///                   is computed to leave this address's `/30` inside the tunnel.
+ (NEPacketTunnelNetworkSettings *)makeWithServerAddress:(NSString *)serverAddress
                                                tunnelIP:(NSString *)tunnelIP
                                                    mode:(NSString *)mode
                                                dnsLocal:(nullable NSString *)dnsLocal
                                               dnsRemote:(nullable NSString *)dnsRemote
                                                     mtu:(NSInteger)mtu
                                             chnrouteURL:(nullable NSURL *)chnrouteURL
                                        serverExclusions:(nullable NSArray<NSString *> *)serverExclusions;
@end
