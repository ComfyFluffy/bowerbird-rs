mod waitgroup;
pub use waitgroup::WaitGroup;

use std::net::TcpListener;

pub fn get_available_port<T>(ra: T) -> Option<u16>
where
    T: IntoIterator<Item = u16>,
{
    for p in ra {
        match TcpListener::bind(("127.0.0.1", p)) {
            Ok(_) => return Some(p),
            Err(_) => {}
        }
    }
    None
}
