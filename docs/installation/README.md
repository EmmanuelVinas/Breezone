# Installing Breezone

*[Version française](README.fr.md)*

Breezone is one Docker image: the server, the web UI and the API of the mobile app. It runs on any
Linux machine on your local network (a NAS, a Raspberry Pi, a mini PC), within 300 MB of memory.

- [Docker](#docker)
- [Synology NAS](synology.md) (step by step, in French)
- [Network](#network)
- [Remote access without VPN](#remote-access-without-vpn)
- [Updates](#updates)
- [Backup](#backup)
- [Settings](#settings)
- [Modules](#modules)

## Docker

The image is published on GitHub Container Registry: `ghcr.io/emmanuelvinas/breezone`
(`linux/amd64` and `linux/arm64`). Tags: `latest`, the version (`0.12.0`) and the minor line
(`0.12`), and `beta`, which follows every version, betas included (`0.12.0-beta.4`).

Create a `breezone` folder with this `docker-compose.yml`, then run `docker compose up -d`:

```yaml
services:
  breezone:
    image: ghcr.io/emmanuelvinas/breezone:latest
    container_name: breezone
    restart: unless-stopped
    # Host network (Linux): Breezone finds the WebServers and devices of your LAN by itself.
    network_mode: host
    environment:
      TZ: Europe/Paris
      # BREEZONE_PORT: "8427"   # if port 8427 is already taken
    volumes:
      - breezone_data:/data
    mem_limit: 300m

volumes:
  breezone_data:
```

Open `http://<your-server>:8427`: the first page creates the administrator account, then offers to
import your Airzone account (optional) or to search for devices on the network.

Without compose:

```bash
docker run -d --name breezone --restart unless-stopped --network host \
  -v breezone_data:/data -e TZ=Europe/Paris ghcr.io/emmanuelvinas/breezone:latest
```

## Network

The server must reach your devices on the local network: the Airzone WebServers (port 3000), and
the Shelly, Hue, Trydan, Hoymiles, Renson, ESPHome, Tasmota… With `network_mode: host` it finds them
by itself. Without it (Docker Desktop, a `bridge` network with `-p 8427:8427`), it cannot guess your
network: set it in *Settings › Installations* (e.g. `192.168.1.0/24`), type it in the search of
*Settings › Modules*, or give the device's address.

HomeKit accessories and the Matter bridge use mDNS (multicast): they need the host network.

## Remote access without VPN

Use a [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/):
the repository's [full `docker-compose.yml`](../../docker-compose.yml) has a `cloudflared` service
(`tunnel` profile, token in `.env`: `CLOUDFLARE_TUNNEL_TOKEN`). No port is opened on your router.
**Never** expose port 3000 of the WebServers: their API has no authentication.

To drive another site from this server, see [Linking two sites](../federation.md).

## Behind a reverse proxy, under a sub-path

Breezone can be published under a path of an existing site, e.g. `https://nas.local/breezone/`,
without its own port or domain name. Point the proxy at `http://<server>:8427/`, with the
WebSocket headers (`Upgrade` and `Connection`) for the live updates.

- If the proxy **removes** the path (`/breezone/zones/3` reaches Breezone as `/zones/3`), there is
  nothing else to set.
- If it **keeps** it (Breezone receives `/breezone/zones/3`), set `BREEZONE_BASE_PATH=/breezone`.
  When unsure, set it anyway: it works in both cases.

Open the address with its trailing slash (`/breezone/`). The mobile app takes the full address
(`https://nas.local/breezone`).

## Updates

**From Breezone itself** (*Settings › Breezone*, administrators): Breezone downloads the new version
from the official repository, checks its signature, backs up the database, and restarts on it; a
version that does not start is undone at once. The container is not recreated. The beta channel
can be joined from the same page. `BREEZONE_SELF_UPDATE=false` removes the button.

Before an update, the page says which installed modules would not work with the new version, and
which version of the library replaces them.

**With Docker**: `docker compose pull && docker compose up -d`. To stay on a given version,
replace `latest` with its number. Pulling a new image from time to time also updates the system of
the image (Alpine, certificates).

## Backup

Everything is in the `breezone_data` volume: the SQLite database, the signing secret, `data.key`
(the key of the stored passwords and pairings: keep it with the database), the installed versions
and modules. *Settings › Breezone* also exports the configuration (no secret, no history).

```bash
docker run --rm -v breezone_data:/data -v "$PWD":/backup alpine tar czf /backup/breezone.tgz -C /data .
```

## Settings

Environment variables, all optional (see also [`.env.sample`](../../.env.sample)).

| Variable | Default | Purpose |
| --- | --- | --- |
| `BREEZONE_PORT` | `8427` | Port of the web UI, the API and the WebSocket |
| `BREEZONE_BASE_PATH` | — | Path the UI is published under, behind a reverse proxy that keeps it in the requests (`/breezone`); see [sub-path](#behind-a-reverse-proxy-under-a-sub-path) |
| `BREEZONE_HTTP_PORT` | `8427` | Published port (repository's compose file) |
| `BREEZONE_VERSION` | `latest` | Image tag (repository's compose file) |
| `TZ` / `BREEZONE_TZ` | `Europe/Paris` | Time zone (schedules, sunrise and sunset) |
| `BREEZONE_SCAN_SUBNET` | the server's /24 | Network searched for devices (also settable in the UI) |
| `BREEZONE_AIRZONE_HOSTS` | — | Installations created at first start (`Home=192.168.1.25,Office=192.168.1.40:3000`) |
| `BREEZONE_ADMIN_USER` / `BREEZONE_ADMIN_PASSWORD` | — | Administrator created at first start (otherwise from the web page) |
| `BREEZONE_POLL_INTERVAL_SECS` | `15` | Reading period of the Airzone WebServers |
| `BREEZONE_DEVICE_READ_MS` | `5000` | Reading period of the other devices (Shelly…) |
| `BREEZONE_HISTORY_INTERVAL_SECS` | `300` | One point of history every… |
| `BREEZONE_HISTORY_RETENTION_DAYS` / `BREEZONE_EVENTS_RETENTION_DAYS` | `90` / `30` | Retention |
| `BREEZONE_ACCESS_TOKEN_TTL_SECS` / `BREEZONE_REFRESH_TOKEN_TTL_DAYS` | `900` / `60` | Token lifetimes (device sessions are sliding) |
| `BREEZONE_JWT_SECRET` | generated in `/data/jwt.secret` | Signing secret |
| `BREEZONE_SELF_UPDATE` | `true` | Updates installed from the web UI |
| `BREEZONE_WEATHER` | `true` | Weather and cities looked up on the internet (Open-Meteo, MeteoAlarm) |
| `BREEZONE_MATTER` / `BREEZONE_MATTER_PORT` / `BREEZONE_MATTER_INTERFACE` | off / `5540` / found | Matter bridge (Google Home, Apple Home, Alexa) |
| `BREEZONE_MODULES_DEV` | — | **Development builds only** (`cargo run`, `scripts/dev.sh`; ignored by the image): a folder of modules being written, installed at each start without a signature. See [Writing a module](../../modules/README.md) |
| `RUST_LOG` | `info,…` | Log level |

## Modules

Breezone drives each brand through a **module**. Those of the common brands come with Breezone;
others are installed from *Settings › Modules › Library*, and load only when a device or an
account uses them.

- **The library** lists the modules published by Breezone, signed like its updates. Only those
  that work with your version can be installed; an update of a module whose devices then stop
  answering is undone at once.
- **Sources of modules**: other libraries can be added (a developer's, a community's), each with
  its own signing key. Their modules show as « unofficial »; they never replace a module of
  Breezone, nor a module installed from another source.
- **A package sent by hand** (`.bzm` with its `.bzm.sig`) is accepted only if one of the sources
  signed it.
- Each module declares what it may reach on the network; it runs in a sandbox, with no access to
  the files of the server.

Writing a module, publishing your own source, trying a module without signing it:
[`modules/README.md`](../../modules/README.md).
