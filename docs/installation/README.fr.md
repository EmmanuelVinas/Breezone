# Installer Breezone

*[English version](README.md)*

Breezone tient dans une image Docker : le serveur, l'interface web et l'API de l'application
mobile. Il tourne sur n'importe quelle machine Linux du réseau local (un NAS, un Raspberry Pi, un
mini-PC), dans 300 Mo de mémoire au plus.

- [Docker](#docker)
- [NAS Synology](synology.md) (pas à pas)
- [Réseau](#réseau)
- [Accès à distance sans VPN](#accès-à-distance-sans-vpn)
- [Mises à jour](#mises-à-jour)
- [Sauvegarde](#sauvegarde)
- [Réglages](#réglages)
- [Modules](#modules)

## Docker

L'image est publiée sur GitHub Container Registry : `ghcr.io/emmanuelvinas/breezone`
(`linux/amd64` et `linux/arm64`). Tags : `latest`, la version (`0.12.0`) et la série (`0.12`), et
`beta`, qui suit toutes les versions, bêtas comprises (`0.12.0-beta.4`).

Créez un dossier `breezone` contenant ce `docker-compose.yml`, puis lancez `docker compose up -d` :

```yaml
services:
  breezone:
    image: ghcr.io/emmanuelvinas/breezone:latest
    container_name: breezone
    restart: unless-stopped
    # Réseau de l'hôte (Linux) : Breezone trouve tout seul les WebServers et les appareils du réseau local.
    network_mode: host
    environment:
      TZ: Europe/Paris
      # BREEZONE_PORT: "8427"   # si le port 8427 est déjà pris
    volumes:
      - breezone_data:/data
    mem_limit: 300m

volumes:
  breezone_data:
```

Ouvrez `http://<votre-serveur>:8427` : la première page crée le compte administrateur, puis propose
d'importer votre compte Airzone (facultatif) ou de rechercher les appareils du réseau.

Sans compose :

```bash
docker run -d --name breezone --restart unless-stopped --network host \
  -v breezone_data:/data -e TZ=Europe/Paris ghcr.io/emmanuelvinas/breezone:latest
```

## Réseau

Le serveur doit joindre vos appareils sur le réseau local : les WebServers Airzone (port 3000), et
les Shelly, Hue, Trydan, Hoymiles, Renson, ESPHome, Tasmota… Avec `network_mode: host`, il les trouve
tout seul. Sans (Docker Desktop, réseau `bridge` avec `-p 8427:8427`), il ne peut pas deviner votre
réseau : indiquez-le dans *Réglages › Installations* (ex. `192.168.1.0/24`), tapez-le dans la
recherche de *Réglages › Modules*, ou donnez l'adresse de l'appareil.

Les accessoires HomeKit et le pont Matter passent par mDNS (multidiffusion) : il leur faut le
réseau de l'hôte.

## Accès à distance sans VPN

Utilisez un [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/) :
le [`docker-compose.yml` complet](../../docker-compose.yml) du dépôt contient un service
`cloudflared` (profil `tunnel`, jeton dans `.env` : `CLOUDFLARE_TUNNEL_TOKEN`). Aucun port n'est
ouvert sur la box. N'exposez **jamais** le port 3000 des WebServers : leur API n'a aucune
authentification.

Pour piloter un autre site depuis ce serveur, voir [Relier deux sites](../federation.md).

## Derrière un proxy inversé, sous un sous-chemin

Breezone peut être publié sous un chemin d'un site existant, par exemple
`https://nas.local/breezone/`, sans port ni nom de domaine à lui. Faites pointer le proxy vers
`http://<serveur>:8427/`, avec les en-têtes WebSocket (`Upgrade` et `Connection`) pour le temps réel.

- Si le proxy **retire** le chemin (`/breezone/zones/3` arrive à Breezone en `/zones/3`), il n'y a
  rien d'autre à régler.
- S'il le **garde** (Breezone reçoit `/breezone/zones/3`), ajoutez `BREEZONE_BASE_PATH=/breezone`.
  Dans le doute, ajoutez-la : elle fonctionne dans les deux cas.

Ouvrez l'adresse avec sa barre finale (`/breezone/`). L'application mobile prend l'adresse complète
(`https://nas.local/breezone`).

## Mises à jour

**Depuis Breezone** (*Réglages › Breezone*, administrateurs) : Breezone télécharge la nouvelle
version depuis le dépôt officiel, vérifie sa signature, sauvegarde la base, puis redémarre dessus ;
une version qui ne démarre pas est annulée aussitôt. Le conteneur n'est pas recréé. Le canal bêta
se rejoint depuis la même page. `BREEZONE_SELF_UPDATE=false` retire le bouton.

Avant une mise à jour, la page dit quels modules installés ne fonctionneraient pas avec la nouvelle
version, et quelle version de la bibliothèque les remplace.

**Avec Docker** : `docker compose pull && docker compose up -d`. Pour rester sur une version
précise, remplacez `latest` par son numéro. Tirer une nouvelle image de temps en temps met aussi à
jour le système de l'image (Alpine, certificats).

## Sauvegarde

Tout est dans le volume `breezone_data` : la base SQLite, le secret de signature, `data.key` (la
clé des mots de passe et appairages gardés : à garder avec la base), les versions et les modules
installés. *Réglages › Breezone* exporte aussi la configuration (sans secret ni historique).

```bash
docker run --rm -v breezone_data:/data -v "$PWD":/backup alpine tar czf /backup/breezone.tgz -C /data .
```

## Réglages

Variables d'environnement, toutes facultatives (voir aussi [`.env.sample`](../../.env.sample)).

| Variable | Par défaut | Rôle |
| --- | --- | --- |
| `BREEZONE_PORT` | `8427` | Port de l'interface web, de l'API et du WebSocket |
| `BREEZONE_BASE_PATH` | — | Chemin sous lequel l'interface est publiée, derrière un proxy inversé qui le garde dans les requêtes (`/breezone`) ; voir [sous-chemin](#derrière-un-proxy-inversé-sous-un-sous-chemin) |
| `BREEZONE_HTTP_PORT` | `8427` | Port publié (compose du dépôt) |
| `BREEZONE_VERSION` | `latest` | Tag de l'image (compose du dépôt) |
| `TZ` / `BREEZONE_TZ` | `Europe/Paris` | Fuseau horaire (programmations, lever et coucher du soleil) |
| `BREEZONE_SCAN_SUBNET` | le /24 du serveur | Réseau où chercher les appareils (aussi réglable dans l'interface) |
| `BREEZONE_AIRZONE_HOSTS` | — | Installations créées au premier démarrage (`Maison=192.168.1.25,Bureau=192.168.1.40:3000`) |
| `BREEZONE_ADMIN_USER` / `BREEZONE_ADMIN_PASSWORD` | — | Administrateur créé au premier démarrage (sinon depuis la page web) |
| `BREEZONE_POLL_INTERVAL_SECS` | `15` | Période de lecture des WebServers Airzone |
| `BREEZONE_DEVICE_READ_MS` | `5000` | Période de lecture des autres appareils (Shelly…) |
| `BREEZONE_HISTORY_INTERVAL_SECS` | `300` | Un point d'historique toutes les… |
| `BREEZONE_HISTORY_RETENTION_DAYS` / `BREEZONE_EVENTS_RETENTION_DAYS` | `90` / `30` | Durées de conservation |
| `BREEZONE_ACCESS_TOKEN_TTL_SECS` / `BREEZONE_REFRESH_TOKEN_TTL_DAYS` | `900` / `60` | Durée des jetons (les sessions des appareils glissent) |
| `BREEZONE_JWT_SECRET` | généré dans `/data/jwt.secret` | Secret de signature |
| `BREEZONE_SELF_UPDATE` | `true` | Mises à jour installées depuis l'interface |
| `BREEZONE_WEATHER` | `true` | Météo et villes consultées sur Internet (Open-Meteo, MeteoAlarm) |
| `BREEZONE_MATTER` / `BREEZONE_MATTER_PORT` / `BREEZONE_MATTER_INTERFACE` | coupé / `5540` / trouvée | Pont Matter (Google Home, Apple Maison, Alexa) |
| `BREEZONE_MODULES_DEV` | — | **Builds de développement seulement** (`cargo run`, `scripts/dev.sh` ; ignoré par l'image) : un dossier de modules en cours d'écriture, installés à chaque démarrage sans signature. Voir [Écrire un module](../../modules/README.md) |
| `RUST_LOG` | `info,…` | Niveau des journaux |

## Modules

Breezone pilote chaque marque par un **module**. Ceux des marques courantes sont livrés avec
Breezone ; d'autres s'installent depuis *Réglages › Modules › Bibliothèque*, et ne se chargent que
lorsqu'un appareil ou un compte s'en sert.

- **La bibliothèque** liste les modules publiés par Breezone, signés comme ses mises à jour. Seuls
  ceux qui fonctionnent avec votre version s'installent ; la mise à jour d'un module dont les
  appareils ne répondent plus est annulée aussitôt.
- **Sources de modules** : d'autres bibliothèques peuvent être ajoutées (celle d'un développeur,
  d'une communauté), chacune avec sa propre clé de signature. Leurs modules sont marqués « non
  officiel » ; ils ne remplacent jamais un module de Breezone, ni un module installé depuis une
  autre source.
- **Un paquet envoyé à la main** (`.bzm` et sa `.bzm.sig`) n'est accepté que si l'une des sources
  l'a signé.
- Chaque module déclare ce qu'il peut joindre sur le réseau ; il tourne dans un bac à sable, sans
  accès aux fichiers du serveur.

Écrire un module, publier sa propre source, essayer un module sans le signer :
[`modules/README.md`](../../modules/README.md) (en anglais).
