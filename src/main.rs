use std::net::UdpSocket;
use std::time::SystemTime;
use pnet::packet::udp::Udp;
use rand::prelude::*;
use serde_derive::{Deserialize, Serialize};
use bincode::Options;
use bincode::config::*;

fn listen(socket: &UdpSocket, buffer: &mut[u8]) -> usize {
    let (number_of_bytes, src_addr) = socket.recv_from(buffer).expect("no data received");
    number_of_bytes
}

fn send(socket: &UdpSocket, receiver: &str, msg: &[u8]) -> usize {
    socket.send_to(msg, receiver).expect("Failed to send message")
}

#[derive(Debug, Serialize)]
struct TrackerConnectionRequest {
    connection_id: u64,
    action: u32,
    transaction_id: u32,
}

#[derive(Debug, Deserialize)]
struct TrackerConnectionResponse {
    action: u32,
    transaction_id: u32,
    connection_id: u64,
}

#[derive(Debug)]
struct TrackerConnection {
    connection_id: u64,
    creation_time: SystemTime,
}

#[derive(Debug, Serialize)]
struct TrackerAnnounceRequest {
    connection_id: u64,
    action: u32,
    transaction_id: u32,
    info_hash: Vec<u8>,
    peer_id: Vec<u8>,
    downloaded: u64,
    left: u64,
    uploaded: u64,
    event: u32,
    ipv4: u32,
    key: u32, // not used
    num_clients: i32,
    port: u16,
}

#[derive(Debug, Deserialize)]
struct TrackerAnnounceResponse {
    action: u32,
    transaction_id: u32,
    interval: u32,
    leechers: u32, 
    seeders: u32,
    peers: Vec<u8>
}

#[derive(Debug, Deserialize)]
struct TrackerErrorResponse {
    action: u32,
    transaction_id: u32,
    message: Vec<char>
}

#[derive(Debug)]
struct MagnetUriData {
    hash_algorithm: String,
    info_hash: String,
    display_name: String,
    trackers: Vec<String>,
}

fn parse_magnet_uri(magnet_uri: &str) -> MagnetUriData {
    // decode percentage encoded chars
    let magnet_uri = urlencoding::decode(magnet_uri).unwrap().into_owned();

    // hash algorithm, infohash
    let xt_index = magnet_uri.find("xt").unwrap();
    let xt = &magnet_uri[xt_index+3..magnet_uri[xt_index..].find("&").unwrap()].split(':').collect::<Vec<&str>>();
    let hash_algorithm = xt[1];
    let info_hash = xt[2];

    // display name
    let dn_index = magnet_uri.find("dn").unwrap();
    let display_name = &magnet_uri[dn_index+3..dn_index+magnet_uri[dn_index..].find('&').unwrap()];

    // trackers
    let mut trackers: Vec<&str> = Vec::new();
    let mut curr = magnet_uri.find("tr=").unwrap();
    while curr < magnet_uri.len() {
        let mut tracker = match magnet_uri[curr..].find('&') {
            None => &magnet_uri[curr+3..], // end of the trackers, no '&' afterwards
            Some(next) => &magnet_uri[curr+3..curr+next],
        };

        if tracker.starts_with("udp://") { // only support UDP trackers (for now)
            tracker = tracker.strip_prefix("udp://").unwrap();
            if tracker.ends_with("/announce") {
                tracker = tracker.strip_suffix("/announce").unwrap();
            }
            trackers.push(tracker);
        }
        match &magnet_uri[curr+3..].find("tr=") {
            None => break,
            Some(next) => {
                curr += *next + 3;
            },
        }        
    }

    MagnetUriData {
        hash_algorithm: hash_algorithm.to_string(),
        info_hash: info_hash.to_string(),
        display_name: display_name.to_string(),
        trackers: trackers.iter().map(|s| s.to_string()).collect()
    }
}

fn connect_to_tracker(socket: &UdpSocket, tracker_url: &String) -> Option<TrackerConnection> {
    let mut rng = rand::thread_rng();
    let bincode = make_custom_bincode();

    let tracker_connection_request = TrackerConnectionRequest {
        connection_id: u64::from_be_bytes([0,0,4,23,39,16,25,128]), // magic constant 0x4
        action: 0,
        transaction_id: rng.next_u32()
    };

    send(&socket, tracker_url, &bincode.serialize(&tracker_connection_request).unwrap());
    let mut recv_buf: Vec<u8> = vec![0; 16];
    listen(&socket, &mut recv_buf);
    println!("{:?}", recv_buf);

    let tracker_connection_response: TrackerConnectionResponse = bincode.deserialize(&recv_buf).unwrap();
    println!("{:?}", tracker_connection_response);
    assert!(tracker_connection_request.transaction_id == tracker_connection_response.transaction_id);
    assert!(tracker_connection_response.action == 0);
    println!("established connection to tracker!");

    Some(
        TrackerConnection {
            connection_id: tracker_connection_response.connection_id,
            creation_time: SystemTime::now()
        }
    )
}

fn announce_to_tracker(socket: &UdpSocket, tracker_url: &String, magnet: &MagnetUriData, tracker_connection: &TrackerConnection) /*-> Option<TrackerAnnounceResponse>*/ {
    let mut rng = rand::thread_rng();
    let bincode = make_custom_bincode();

    let announce_request: TrackerAnnounceRequest = TrackerAnnounceRequest {
        connection_id: tracker_connection.connection_id,
        action: 1,
        transaction_id: rng.next_u32(),
        info_hash: magnet.info_hash.as_bytes().to_vec(),
        peer_id: "8466F12337DB7CD0AC5F9EA5D8EDBE0858C086CE".as_bytes().to_vec(),
        downloaded: 0,
        left: 0,
        uploaded: 0,
        event: 0,
        ipv4: 0,
        key: 0,
        num_clients: 100,
        port: 6969
    };
    send(&socket, tracker_url, &bincode.serialize(&announce_request).unwrap());
    let mut recv_buf: Vec<u8> = vec![0; 500];
    listen(&socket, &mut recv_buf);
    println!("{:?}", recv_buf);

    assert!(u32::from_be_bytes(recv_buf[4..8]) == announce_request.transaction_id);

    match recv_buf[..4] {
        [0,0,0,3] => {
            let message = std::str::from_utf8(&recv_buf[8..]).unwrap();
            println!("error: {}", message);
        },
        _ => {
            println!("not an error");
            // println!("{:?}", recv_buf);

            let interval = u32::from_be_bytes(recv_buf[8..12].try_into().unwrap());
            println!("interval: {}", interval);

            let leechers = u32::from_be_bytes(recv_buf[12..16].try_into().unwrap());
            println!("leechers: {}", leechers);

            let seeders = u32::from_be_bytes(recv_buf[16..20].try_into().unwrap());
            println!("seeders: {}", seeders);

            // let tracker_announce_response: TrackerAnnounceResponse = bincode.deserialize(&recv_buf).unwrap();
            // println!("{:?}", tracker_announce_response);
        },
    }
}

fn make_custom_bincode() -> bincode::config::WithOtherIntEncoding<bincode::config::WithOtherEndian<bincode::DefaultOptions, bincode::config::BigEndian>, bincode::config::FixintEncoding> {
    bincode::DefaultOptions::new().with_big_endian().with_fixint_encoding()
}

fn main () {
    let args: Vec<String> = std::env::args().collect();
    let magnet_uri = args.get(1).expect("please provide a magnet uri argument");
    let magnet: MagnetUriData = parse_magnet_uri(magnet_uri);
    if magnet.hash_algorithm != "btih" {
        panic!("Hash algorithm '{}' not supported. Can't continue.", magnet.hash_algorithm);
    }
    if magnet.trackers.is_empty() {
        panic!("No trackers found. Can't continue.");
    }

    println!("Using magnet data: {:?}", magnet);

    let mut rng = rand::thread_rng();
    let bincode = make_custom_bincode();
    let socket = UdpSocket::bind("0.0.0.0:0").expect("failed to bind host socket");

    let tracker_url = magnet.trackers.get(1).unwrap();
    let tracker_connection = connect_to_tracker(&socket, &tracker_url).expect("Failed to connect to the tracker");
    println!("{:?}", tracker_connection);

    let announce_response = announce_to_tracker(&socket, tracker_url, &magnet, &tracker_connection);
}