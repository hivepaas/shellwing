//! Port forwarding: -L, -R and -D as given on the command line, and a forward
//! added from the palette, each carrying real traffic.

use std::io::{Read, Write};

use super::server::echo_service;
use super::{Policy, STEP, Ui, free_port, scenario, talk};

/// A server that carries forwards, trusted.
fn forwarding_server(ui: &mut Ui) -> super::Server {
    let server = ui.server(Policy {
        forward: true,
        ..Policy::password()
    });
    ui.trust(&server);
    server
}

scenario!(
    /// -L: a local port leads, through the server, to a service beside it.
    local_forward_carries_traffic,
    |ui| {
        let service = echo_service(&ui.rt);
        let server = forwarding_server(ui);
        let local = free_port();
        let spec = format!("-L {local}:127.0.0.1:{service}");
        ui.open_with(Some(&server.spec()), &[spec.as_str()]);
        ui.login();
        ui.wait_for(&format!("forwarding localhost:{local}"));
        assert_eq!(talk(local, "through the tunnel"), "echo: through the tunnel");
        assert_eq!(talk(local, "and again"), "echo: and again");
        assert!(server.seen(|s| s.forwards.contains(&format!("127.0.0.1:{service}"))));
    }
);

scenario!(
    /// -R: a port on the server leads back to a service on this computer.
    remote_forward_carries_traffic,
    |ui| {
        let service = echo_service(&ui.rt);
        let server = forwarding_server(ui);
        let remote = free_port();
        let spec = format!("-R {remote}:127.0.0.1:{service}");
        ui.open_with(Some(&server.spec()), &[spec.as_str()]);
        ui.login();
        ui.wait_for("forwarding the server's");
        assert_eq!(talk(remote, "from the server side"), "echo: from the server side");
    }
);

scenario!(
    /// -D: a SOCKS proxy; a SOCKS5 client reaches a service through it.
    socks_proxy_carries_traffic,
    |ui| {
        let service = echo_service(&ui.rt);
        let server = forwarding_server(ui);
        let socks = free_port();
        let spec = format!("-D {socks}");
        ui.open_with(Some(&server.spec()), &[spec.as_str()]);
        ui.login();
        ui.wait_for(&format!("SOCKS proxy on localhost:{socks}"));

        let mut s = std::net::TcpStream::connect(("127.0.0.1", socks)).expect("connect");
        s.set_read_timeout(Some(STEP)).expect("timeout");
        // Hello: version 5, one method, no authentication.
        s.write_all(&[5, 1, 0]).expect("hello");
        let mut reply = [0u8; 2];
        s.read_exact(&mut reply).expect("hello reply");
        assert_eq!(reply, [5, 0]);
        // CONNECT 127.0.0.1:service.
        let p = service.to_be_bytes();
        s.write_all(&[5, 1, 0, 1, 127, 0, 0, 1, p[0], p[1]])
            .expect("connect request");
        let mut reply = [0u8; 10];
        s.read_exact(&mut reply).expect("connect reply");
        assert_eq!(reply[1], 0, "the proxy refused: {reply:?}");
        s.write_all(b"over socks\n").expect("write");
        let mut answer = vec![0u8; "echo: over socks\n".len()];
        s.read_exact(&mut answer).expect("read");
        assert_eq!(String::from_utf8_lossy(&answer), "echo: over socks\n");
    }
);

scenario!(
    /// A local port another program holds: the forward says why it could not
    /// start, and the session still works.
    local_port_already_in_use,
    |ui| {
        let service = echo_service(&ui.rt);
        let server = forwarding_server(ui);
        let busy = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let local = busy.local_addr().expect("addr").port();
        let spec = format!("-L {local}:127.0.0.1:{service}");
        ui.open_with(Some(&server.spec()), &[spec.as_str()]);
        ui.login();
        ui.wait_for("cannot forward");
        ui.line("still usable");
        ui.wait_for("you said: still usable");
        drop(busy);
    }
);

scenario!(
    /// A server that does not allow forwarding: the -L port opens, but a
    /// connection through it is refused, and the session is unharmed.
    server_refuses_forwarding,
    |ui| {
        let service = echo_service(&ui.rt);
        let server = ui.server(Policy::password());
        ui.trust(&server);
        let local = free_port();
        let spec = format!("-L {local}:127.0.0.1:{service}");
        ui.open_with(Some(&server.spec()), &[spec.as_str()]);
        ui.login();
        ui.wait_for(&format!("forwarding localhost:{local}"));
        let mut s = std::net::TcpStream::connect(("127.0.0.1", local)).expect("connect");
        s.set_read_timeout(Some(STEP)).expect("timeout");
        let _ = s.write_all(b"hello\n");
        let mut buf = [0u8; 16];
        let n = s.read(&mut buf).unwrap_or(0);
        assert_eq!(n, 0, "data came back through a refused channel");
        ui.line("unharmed");
        ui.wait_for("you said: unharmed");
    }
);

scenario!(
    /// A forward added while connected, from the palette's action.
    forward_added_from_the_palette,
    |ui| {
        let service = echo_service(&ui.rt);
        let server = forwarding_server(ui);
        ui.connect(&server.spec());
        ui.login();
        let local = free_port();
        ui.palette(">forward");
        ui.wait_until("the palette's question", |ui| ui.ws(|ws, _, _| ws.palette_open()));
        ui.line(&format!("-L {local}:127.0.0.1:{service}"));
        ui.wait_for_notice("Forwarding");
        assert_eq!(talk(local, "added later"), "echo: added later");
    }
);

scenario!(
    /// A LocalForward in the ssh config's entry for the host starts with it,
    /// as `ssh box` would.
    local_forward_from_the_ssh_config,
    |ui| {
        let service = echo_service(&ui.rt);
        let server = forwarding_server(ui);
        let local = free_port();
        ui.ssh_config(&format!(
            "Host box\n  HostName 127.0.0.1\n  Port {}\n  User {}\n  LocalForward {local} 127.0.0.1:{service}\n",
            server.port,
            crate::ui_tests::USER
        ));
        ui.connect("box");
        ui.login();
        ui.wait_for(&format!("forwarding localhost:{local}"));
        assert_eq!(talk(local, "from the config"), "echo: from the config");
    }
);

scenario!(
    /// A forward added in the host editor starts with the host.
    forward_added_in_the_editor_starts_with_the_host,
    |ui| {
        let service = echo_service(&ui.rt);
        let server = forwarding_server(ui);
        let local = free_port();
        ui.config_file(
            "hosts.toml",
            &format!(
                "[[host]]\nname = \"web\"\naddress = \"127.0.0.1\"\nport = {}\nuser = \"{}\"\n",
                server.port,
                crate::ui_tests::USER
            ),
        );
        ui.open();
        ui.hover("host:web");
        ui.click("edit-host:web");
        ui.wait_until("the host editor", |ui| ui.host_editor_open());
        ui.click("add-forward");
        ui.type_text(&format!("-L {local}:127.0.0.1:{service}"));
        ui.keys("enter");
        ui.keys("escape");
        ui.click("save");
        ui.wait_until("the editor to close", |ui| !ui.host_editor_open());
        let hosts = std::fs::read_to_string(ui.config_dir().join("hosts.toml")).unwrap_or_default();
        assert!(hosts.contains(&format!("\"-L {local}:127.0.0.1:{service}\"")), "{hosts}");
        ui.click("host:web");
        ui.login();
        ui.wait_for(&format!("forwarding localhost:{local}"));
        assert_eq!(talk(local, "set in the editor"), "echo: set in the editor");
    }
);
