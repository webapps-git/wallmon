//! UPS state from apcupsd over its NIS port (the protocol `apcaccess` uses).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use crate::state::Ups;

const TIMEOUT: Duration = Duration::from_secs(3);

pub struct Reader {
    address: String,
}

impl Reader {
    pub fn new(address: String) -> Self {
        Reader { address }
    }

    pub fn read(&mut self) -> Result<Ups, String> {
        let addr = self
            .address
            .to_socket_addrs()
            .map_err(|e| format!("{}: {e}", self.address))?
            .next()
            .ok_or(format!("{}: brak adresu", self.address))?;
        let mut s = TcpStream::connect_timeout(&addr, TIMEOUT).map_err(|e| format!("apcupsd {}: {e}", self.address))?;
        s.set_read_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
        s.set_write_timeout(Some(TIMEOUT)).map_err(|e| e.to_string())?;
        // Every message is a 2-byte big-endian length followed by text; length 0 ends the reply.
        s.write_all(&[0, 6]).and_then(|_| s.write_all(b"status")).map_err(|e| e.to_string())?;
        let mut lines = vec![];
        loop {
            let mut len = [0u8; 2];
            s.read_exact(&mut len).map_err(|e| format!("apcupsd: {e}"))?;
            let n = u16::from_be_bytes(len) as usize;
            if n == 0 {
                break;
            }
            let mut line = vec![0u8; n];
            s.read_exact(&mut line).map_err(|e| format!("apcupsd: {e}"))?;
            lines.push(String::from_utf8_lossy(&line).into_owned());
        }
        parse(&lines.concat())
    }
}

/// Parses `KEY : value` lines as printed by `apcaccess`.
pub fn parse(text: &str) -> Result<Ups, String> {
    let f: HashMap<&str, &str> = text
        .lines()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            Some((k.trim(), v.trim()))
        })
        .collect();
    let num = |k: &str| f.get(k).and_then(|v| v.split_whitespace().next()?.parse::<f64>().ok());
    let status = f.get("STATUS").ok_or("apcupsd: brak STATUS")?;
    Ok(Ups {
        model: f.get("MODEL").map_or_else(|| "UPS".into(), |m| m.to_string()),
        on_battery: status.contains("ONBATT"),
        on_battery_for: Duration::from_secs_f64(num("TONBATT").unwrap_or(0.0)),
        load: num("LOADPCT").unwrap_or(0.0),
        charge: num("BCHARGE").unwrap_or(0.0),
        time_left: Duration::from_secs_f64(num("TIMELEFT").unwrap_or(0.0) * 60.0),
        line_voltage: num("LINEV").unwrap_or(0.0),
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_apcaccess_output() {
        let u = super::parse(
            "APC      : 001,036,0868\nMODEL    : Back-UPS BX950MI  \nSTATUS   : ONLINE \nLINEV    : 230.0 Volts\n\
             LOADPCT  : 21.0 Percent\nBCHARGE  : 100.0 Percent\nTIMELEFT : 26.9 Minutes\nTONBATT  : 0 Seconds\n",
        )
        .unwrap();
        assert_eq!(u.model, "Back-UPS BX950MI");
        assert!(!u.on_battery);
        assert_eq!((u.load, u.charge, u.line_voltage), (21.0, 100.0, 230.0));
        assert_eq!(u.time_left.as_secs(), 1614);
        assert!(super::parse("STATUS   : ONBATT \n").unwrap().on_battery);
    }
}
