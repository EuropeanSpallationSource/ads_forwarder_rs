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

use std::fmt::{self, Display};
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use byteorder::{ByteOrder, LittleEndian as LE};
use itertools::Itertools;
use interfaces;

pub const BECKHOFF_BC_UDP_PORT: u16 = 48847; // 0xBECF
pub const BECKHOFF_TCP_PORT:    u16 = 48898; // 0xBF02
pub const BECKHOFF_UDP_PORT:    u16 = 48899; // 0xBF03
pub const BECKHOFF_UDP_MAGIC:   u32 = 0x71146603;


fn printable(ch: &u8) -> char {
    if *ch >= 32 && *ch <= 127 { *ch as char } else { '.' }
}

pub fn hexdump(mut data: &[u8]) {
    let mut addr = 0;
    while !data.is_empty() {
        let (line, rest) = data.split_at(data.len().min(16));
        println!("{:#06x}: {:02x}{} | {}", addr,
                 line.iter().format(" "),
                 (0..16 - line.len()).map(|_| "   ").format(""),
                 line.iter().map(printable).format(""));
        addr += 16;
        data = rest;
    }
    println!();
}

pub fn force_ipv4(addr: IpAddr) -> Ipv4Addr {
    match addr {
        IpAddr::V6(_) => panic!("IPv4 address required"),
        IpAddr::V4(ip) => ip
    }
}

pub fn in_same_net<T: Into<u32>>(addr1: T, addr2: T, netmask: T) -> bool {
    let (addr1, addr2, netmask) = (addr1.into(), addr2.into(), netmask.into());
    addr1 & netmask == addr2 & netmask
}

pub fn ipv4_addr(addresses: &[interfaces::Address]) -> Option<(Ipv4Addr, Ipv4Addr)> {
    addresses.iter().find(|ad| ad.kind == interfaces::Kind::Ipv4)
                    .map(|ad| (force_ipv4(ad.addr.unwrap().ip()),
                               force_ipv4(ad.mask.unwrap().ip())))
}


#[derive(Clone, PartialEq, Eq, Default)]
pub struct AmsNetId(pub [u8; 6]);

impl AmsNetId {
    pub fn is_empty(&self) -> bool {
        self.0 == [0, 0, 0, 0, 0, 0]
    }

    pub fn from_slice(slice: &[u8]) -> Self {
        debug_assert!(slice.len() == 6);
        let mut arr = [0; 6];
        arr.copy_from_slice(slice);
        AmsNetId(arr)
    }
}

impl FromStr for AmsNetId {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<AmsNetId, &'static str> {
        // Not given parts of NetID default to "1"
        let mut arr = [1; 6];
        for (i, part) in s.split('.').enumerate() {
            match (arr.get_mut(i), part.parse()) {
                (Some(loc), Ok(byte)) => *loc = byte,
                _ => return Err("invalid NetID string"),
            }
        }
        Ok(AmsNetId(arr))
    }
}

impl Display for AmsNetId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0.iter().format("."))
    }
}

pub struct AdsMessage(pub Vec<u8>);

impl AdsMessage {
    pub fn new(msg: Vec<u8>) -> AdsMessage {
        let msg = AdsMessage(msg);
        // XXX expand checks
        assert!(msg.length() == msg.0.len());
        msg
    }

    pub fn length(&self) -> usize {
        6 + LE::read_u32(&self.0[2..6]) as usize
    }

    pub fn dest_id(&self) -> AmsNetId {
        AmsNetId::from_slice(&self.0[6..12])
    }

    pub fn source_id(&self) -> AmsNetId {
        AmsNetId::from_slice(&self.0[14..20])
    }

    pub fn patch_dest_id(&mut self, id: &AmsNetId) {
        self.0[6..12].copy_from_slice(&id.0);
    }

    pub fn patch_source_id(&mut self, id: &AmsNetId) {
        self.0[14..20].copy_from_slice(&id.0);
    }
}
