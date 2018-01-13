// *****************************************************************************
//
// This program is free software; you can redistribute it and/or modify it under
// the terms of the GNU General Public License as published by the Free Software
// Foundation; either version 2 of the License, or (at your option) any later
// version.
//
// This program is distributed in the hope that it will be useful, but WITHOUT
// ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS
// FOR A PARTICULAR PURPOSE.  See the GNU General Public License for more
// details.
//
// You should have received a copy of the GNU General Public License along with
// this program; if not, write to the Free Software Foundation, Inc.,
// 59 Temple Place, Suite 330, Boston, MA  02111-1307  USA
//
// Module authors:
//   Enrico Faulhaber <enrico.faulhaber@frm2.tum.de>
//   Georg Brandl <g.brandl@fz-juelich.de>
//
// *****************************************************************************

use std::error::Error;
use std::collections::HashMap;
use std::net::{UdpSocket, Ipv4Addr};
use std::time::Duration;

use forwarder::Beckhoff;
use util::{AmsNetId, hexdump, find_ipv4_addrs, unwrap_ipv4, in_same_net, FWDER_NETID,
           BECKHOFF_BC_UDP_PORT, BECKHOFF_UDP_PORT, UdpMessage};


/// Determines what to scan.
pub enum Scan<'a> {
    Everything,
    Interface(&'a str),
    Address(Ipv4Addr),
}


pub struct Scanner {
    dump: bool,
    if_addrs: HashMap<String, (Ipv4Addr, Ipv4Addr)>,
}

impl Scanner {
    pub fn new(dump: bool) -> Scanner {
        Scanner { dump, if_addrs: find_ipv4_addrs() }
    }

    pub fn if_exists(&self, if_name: &str) -> bool {
        self.if_addrs.contains_key(if_name)
    }

    /// Scan the locally reachable network for Beckhoffs.
    ///
    /// If given a `Scan::Interface`, only IPs on that interface are scanned.
    /// If given a `Scan::Address`, only that IP is scanned.
    ///
    /// Returns a vector of found Beckhoffs.
    pub fn scan(&self, what: Scan) -> Vec<Beckhoff> {
        match self.scan_inner(what) {
            Ok(v) => v,
            Err(e) => {
                error!("during scan: {}", e);
                Vec::new()
            }
        }
    }

    fn scan_inner(&self, what: Scan) -> Result<Vec<Beckhoff>, Box<Error>> {
        let broadcast = [255, 255, 255, 255].into();
        match what {
            Scan::Address(bh_addr) =>
                self.scan_addr([0, 0, 0, 0].into(), bh_addr, true),
            Scan::Interface(if_name) =>
                self.scan_addr(self.if_addrs[if_name].0, broadcast, false),
            Scan::Everything => {
                let mut all = Vec::new();
                for &(if_addr, _) in self.if_addrs.values() {
                    all.extend(self.scan_addr(if_addr, broadcast, false)?);
                }
                Ok(all)
            }
        }
    }

    fn scan_addr(&self, bind_addr: Ipv4Addr, send_addr: Ipv4Addr, single_reply: bool)
                 -> Result<Vec<Beckhoff>, Box<Error>> {
        let bc_scan_struct = structure!("<IHHHHHH");
        let bc_scan_result_struct = structure!("<I6x6S6x20s");

        let udp = UdpSocket::bind((bind_addr, 0))?;
        udp.set_broadcast(true)?;
        udp.set_read_timeout(Some(Duration::from_millis(500)))?;

        // scan for BCs: request 3 words from 0:21 (NetID) and 10 words from 100:4 (Name)
        let bc_msg = bc_scan_struct.pack(1, 0, 0x21, 3, 100, 4, 10).unwrap();
        udp.send_to(&bc_msg, (send_addr, BECKHOFF_BC_UDP_PORT))?;
        debug!("scan: sending BC UDP packet");
        if self.dump {
            hexdump(&bc_msg);
        }

        // scan for CXs: "identify" operation in the UDP protocol
        let cx_msg = UdpMessage::new(UdpMessage::IDENTIFY, &FWDER_NETID, 10000, 0);
        udp.send_to(&cx_msg.0, (send_addr, BECKHOFF_UDP_PORT))?;
        debug!("scan: sending CX UDP packet");
        if self.dump {
            hexdump(&cx_msg.0);
        }

        // wait for replies
        let mut beckhoffs = Vec::new();
        let mut reply = [0; 2048];
        while let Ok((len, reply_addr)) = udp.recv_from(&mut reply) {
            let reply = &reply[..len];
            if self.dump {
                info!("scan: reply from {}", reply_addr);
                hexdump(reply);
            }
            let bh_addr = unwrap_ipv4(reply_addr.ip());
            if reply_addr.port() == BECKHOFF_BC_UDP_PORT {
                if let Ok((_, netid, name)) = bc_scan_result_struct.unpack(reply) {
                    let netid = AmsNetId::from_slice(&netid);
                    info!("scan: found {} ({}) at {}",
                          String::from_utf8_lossy(&name), netid, bh_addr);
                    beckhoffs.push(Beckhoff { if_addr: self.find_if_addr(bh_addr),
                                              is_bc: true, bh_addr, netid });
                }
            } else if let Ok((netid, info)) = UdpMessage::parse(reply, UdpMessage::IDENTIFY) {
                let name = info[&UdpMessage::HOST];
                let name = String::from_utf8_lossy(&name[..name.len() - 1]);
                let ver = info[&UdpMessage::VERSION];
                info!("scan: found {}, TwinCat {}.{}.{} ({}) at {}",
                      name, ver[0], ver[1], ver[2] as u16 | (ver[3] as u16) << 8,
                      netid, bh_addr);
                beckhoffs.push(Beckhoff { if_addr: self.find_if_addr(bh_addr),
                                          is_bc: false, bh_addr, netid });
            }
            // if scanning a single address, don't wait for more replies
            if single_reply {
                break;
            }
        }
        Ok(beckhoffs)
    }

    /// Find the local address of the interface whose network contains given addr.
    fn find_if_addr(&self, bh_addr: Ipv4Addr) -> Ipv4Addr {
        for &(if_addr, if_mask) in self.if_addrs.values() {
            if in_same_net(bh_addr, if_addr, if_mask) {
                return if_addr;
            }
        }
        panic!("Did not find local interface address for Beckhoff {}?!", bh_addr);
    }
}
