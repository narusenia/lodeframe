// SPDX-License-Identifier: Apache-2.0 OR MIT
//! What the library logs, read back through a subscriber that writes into a buffer.
//!
//! One test, because the subscriber is global to the process.

use std::{
    cell::Cell,
    io,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use lodeframe::{
    clock::Clock,
    instance::{Instance, Message, Runner},
    net::{Config, Connection, serve},
    protocol::{Error, VarInt, packets::handshake::Intention},
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
    time::sleep,
};

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl io::Write for Buffer {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buffer {
    type Writer = Buffer;

    fn make_writer(&'a self) -> Buffer {
        self.clone()
    }
}

/// Fake time that the instance moves: its one tick takes 300 ms.
#[derive(Clone)]
struct Jumpy(Instant, Rc<Cell<Duration>>);

impl Clock for Jumpy {
    fn now(&self) -> Instant {
        self.0 + self.1.get()
    }

    fn sleep_until(&self, t: Instant) {
        self.1.set(self.1.get().max(t - self.0));
    }
}

struct SlowOnce(Jumpy, usize, Arc<AtomicBool>);

impl Instance for SlowOnce {
    fn handle(&mut self, _: Message) {}

    fn tick(&mut self) {
        self.1 += 1;
        if self.1 == 1 {
            self.0.1.set(self.0.1.get() + Duration::from_millis(300));
        }
        if self.1 == 8 {
            self.2.store(true, Ordering::Relaxed);
        }
    }
}

#[tokio::test]
async fn connection_failures_are_logged_with_the_peer() {
    let buffer = Buffer::default();
    tracing_subscriber::fmt()
        .with_writer(buffer.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .init();

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(serve(listener, Config::default(), |_conn, _, _| async {
        Err(Error::InvalidValue("boom"))
    }));

    // a failed handshake
    let mut bad = TcpStream::connect(addr).await.unwrap();
    bad.write_all(&[0xff, 0xff, 0xff, 0xff]).await.unwrap();
    // a handler that fails for a reason other than the peer leaving
    let mut good = Connection::new(
        TcpStream::connect(addr).await.unwrap(),
        Duration::from_secs(5),
    );
    good.write_packet(&Intention {
        protocol_version: VarInt(0),
        server_address: String::new(),
        server_port: 0,
        next_state: VarInt(2),
    })
    .await
    .unwrap();
    sleep(Duration::from_millis(300)).await;

    // a tick that runs late
    let clock = Jumpy(Instant::now(), Rc::default());
    let stop = Arc::new(AtomicBool::new(false));
    let (mut runner, _handle) = Runner::new(SlowOnce(clock.clone(), 0, stop.clone()));
    runner.run(&clock, &stop);

    let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
    assert!(
        log.contains("DEBUG") && log.contains("handshake failed"),
        "{log}"
    );
    assert!(
        log.contains("WARN") && log.contains("connection failed") && log.contains("boom"),
        "{log}"
    );
    assert!(
        log.contains("WARN") && log.contains("tick behind, catching up"),
        "{log}"
    );
    // the span carries the peer address
    assert!(log.contains("conn{peer=127.0.0.1:"), "{log}");
}
