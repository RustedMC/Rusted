# Hosting on a VPS

These steps are for Debian or Ubuntu. Other Linux distros work the same way with their own
package manager.

## 1. Build

```bash
sudo apt install build-essential git
curl https://sh.rustup.rs -sSf | sh
git clone https://github.com/RustedMC/Rusted.git && cd Rusted
cargo build --release
```

## 2. Install as a service

Run it as its own user, so a bug in the server can't touch the rest of the machine:

```bash
sudo useradd -r -m -d /opt/rusted rusted
sudo cp target/release/rusted /opt/rusted/ && sudo cp -r data /opt/rusted/
sudo chown -R rusted: /opt/rusted
sudo cp deploy/rusted.service /etc/systemd/system/
sudo systemctl enable --now rusted
```

The service starts on boot and restarts by itself if the server crashes. Stopping it saves
the world and players first.

## 3. Open the port

```bash
sudo ufw allow 19132
```

Open both **UDP and TCP** `19132`, here and in your provider's firewall panel if it has one.
Newer clients join over TCP, older ones over UDP.

## Everyday use

| Task | Command |
|---|---|
| Watch the logs | `journalctl -u rusted -f` |
| Restart | `sudo systemctl restart rusted` |
| Change settings | edit `/opt/rusted/server.properties`, then restart |

## Updating

```bash
cd Rusted && git pull && cargo build --release
sudo cp target/release/rusted /opt/rusted/
sudo systemctl restart rusted
```

## Hosting at home

The same steps work on any Linux machine. Friends outside your network also need you to
forward port `19132` (UDP and TCP) on your router to that machine.
