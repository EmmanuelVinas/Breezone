# API Breezone

Base : `http(s)://<serveur>` — JSON, champs en `snake_case`. Erreurs : statut HTTP + `{"error": "…"}`.

Authentification : `Authorization: Bearer <access_token>`. Le jeton d'accès dure 15 min ; on en obtient
un nouveau avec le refresh token de l’appareil.
🔒 = authentifié, 👑 = administrateur du serveur, 🛠 = administrateur du serveur ou de l'installation.

## Versions de l'API

`/health` et `/api/version` annoncent `api_version` (version de l'API du serveur) et `min_client_api`
(plus ancienne version de client encore servie). L'application envoie la sienne dans l'en-tête
`Breezone-Api` ; un client plus ancien que `min_client_api` reçoit `426 Upgrade Required` sur `/api/*`.
Sans cet en-tête (navigateur, scripts, anciennes applications), la requête est servie.

Règles de compatibilité :
- Ajouter une route ou un champ ne change pas `api_version` : les clients ignorent les champs inconnus,
  et un champ absent garde sa valeur par défaut.
- Un changement incompatible (champ retiré ou renommé, sens modifié) augmente `api_version`. Le serveur
  continue de servir l'ancienne forme tant que possible ; `min_client_api` n'augmente que lorsqu'elle
  est vraiment abandonnée.
- L'application connaît la plus ancienne API serveur avec laquelle elle fonctionne : en dessous, elle
  demande de mettre le serveur à jour ; au-dessus de la sienne, elle propose de se mettre à jour.
- Un serveur qui n'annonce pas `api_version` (jusqu'à 0.4.x) parle l'API 1.

### Capacités (ADR 0012)

`GET 🔒 /api/capabilities` → ce que le serveur sait faire, fonction par fonction :
`{breezone, channel, protocol:{current, min}, traits:{"climate.zone":[1]}, features:{"federation.watch":1, …}, modules:[{id, version, provides}], caps_rev}`.
`caps_rev` (empreinte du document) figure aussi dans `/api/version` : un client relit les capacités
quand elle change, et n'active que les écrans des fonctions annoncées. Les serveurs fédérés
s'échangent ce même document (`POST /api/federation/hello`).

Une commande portant un champ que le serveur ne connaît pas n'est jamais appliquée en partie :
`422 {error, code:"unsupported", unsupported:[champs], breezone}`. L'émetteur sait ainsi qu'il faut
mettre ce serveur à jour.

## Langue des messages

Les textes écrits par le serveur pour les personnes (`error` des réponses, messages du journal,
erreurs de connexion et de synchronisation, notes) suivent l'en-tête `Accept-Language` de la requête
(`fr`, `en` ou `es` ; anglais par défaut). Sur le WebSocket, c'est `lang` dans le message
d'authentification. Le journal est enregistré dans les trois langues. Quelques messages lus par des
programmes restent en anglais : `invalid username or password` (le web et l'application affichent
leur propre texte), `share expired`, `share revoked` et `pairing pending approval` (lus par l'autre
serveur).

## Premier démarrage et sessions

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET | `/health` | `{"status":"ok","version":"…","api_version":1,"min_client_api":1}` |
| GET | `/api/setup` | `{"needs_setup": bool}` — vrai tant qu'aucun compte n'existe |
| POST | `/api/setup` | `{username, password, device_name?}` → crée l'administrateur, renvoie un `TokenPair` |
| POST | `/api/auth/login` | `{username, password, device_name?}` → `TokenPair` (verrou 15 min après 5 échecs) |
| POST | `/api/auth/refresh` | `{refresh_token}` → nouveau `TokenPair` ; l'ancien refresh token reste valable 120 s |
| POST | `/api/auth/logout` | `{refresh_token}` → révoque l'appareil |
| GET 🔒 | `/api/me` | Utilisateur courant, avec `access` : installations visibles (`null` pour un administrateur), `rights` : ce qu'il peut modifier sur au moins une installation (pour les applications jusqu'à 0.7.0 ; chaque `Installation` porte ses propres `rights`), et `language` : langue choisie dans le profil (`"fr"`, `"en"`, `"es"` ou `null` pour suivre le navigateur) |
| PUT 🔒 | `/api/me/language` | `{language: "fr"\|"en"\|"es"\|null}` — langue du profil, appliquée par l'interface web |
| PUT 🔒 | `/api/me/password` | `{current_password, new_password}` — déconnecte les autres appareils |
| GET 🔒 | `/api/sessions` | Appareils connectés (`current` = celui-ci) |
| DELETE 🔒 | `/api/sessions/{id}` | Déconnecte un appareil (immédiat, WebSocket compris) |

`TokenPair` : `{access_token, refresh_token, token_type, expires_in, refresh_expires_at, user}`.

## Utilisateurs 👑

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET | `/api/users` | Liste, avec `access:[{installation_id, permission:"read"\|"control"\|"admin", rights:{zones, routines, schedules}}]` |
| POST | `/api/users` | `{username, password, role: "admin"\|"user", access?}` (`rights` d'une entrée : tout autorisé par défaut) |
| PUT | `/api/users/{id}` | `{role?, password?, access?}` — un nouveau mot de passe déconnecte l'utilisateur partout ; `access` remplace les installations données et leurs droits |
| DELETE | `/api/users/{id}` | Impossible pour soi-même ou le dernier administrateur |

Un administrateur voit et pilote toutes les installations. Un utilisateur (`role: "user"`) ne voit
que celles de son `access` : les autres sont absentes de toutes les réponses et du WebSocket (404 si
on les demande), une installation en `read` est envoyée avec `controllable: false` et ses commandes
répondent 403. Les minuteries et routines ne peuvent viser que des installations pilotables par
l'utilisateur (la cible « toutes » est réservée aux administrateurs). Une installation ajoutée
ensuite (à la main, depuis le compte Airzone ou via un autre nœud) n'est donnée à personne.

Les droits se donnent par installation. `read` : consulter ; `control` : piloter, et modifier ce
que les `rights` de l'entrée autorisent (`{zones, routines, schedules}`, trois booléens, tous à
`true` par défaut) ; `admin` : tous les droits sur l'installation, y compris ses réglages (🛠 :
adresse, lien Airzone, suppression). Un administrateur du serveur a tous les droits partout.
`zones` : nom, groupe et ordre des zones (`…/meta`) ; `routines` :
créer, modifier et supprimer des routines (les lancer ne demande que le pilotage) ; `schedules` :
créer, modifier et supprimer des programmations Airzone (les activer ou désactiver ne demande que
le pilotage). Sans le droit, ces requêtes répondent 403. Chaque `Installation` renvoyée porte
`rights: {zones, routines, schedules, admin}`, ce que l'utilisateur qui la lit peut y modifier. Les
réglages du serveur (ajout d'installation, compte Airzone, recherche réseau, nœuds, partages,
utilisateurs : 👑) restent réservés aux administrateurs du serveur. Un changement d'accès ou de droits envoie un nouveau `snapshot` sur le WebSocket
de l'utilisateur ; les clients relisent alors `/api/me`.

## Installations et zones

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/status` | État complet : `[Installation]` |
| GET 🔒 | `/api/installations/{id}` | Une `Installation` |
| POST 🔒 | `/api/installations/{id}/refresh` | Interroge immédiatement le WebServer |
| GET 🔒 | `/api/installations` | Configuration (`host`, `port`, `mac`, `location`, `enabled`, `cloud_id`, `node_id`, `remote_id`, `record_demand`) |
| POST 👑 | `/api/installations` | `{name, host, port?=3000, enabled?=true, record_demand?=false}` |
| PUT 🛠 | `/api/installations/{id}` | Idem, `record_demand` inchangé s'il est absent (installation via un nœud : seuls `name` et `enabled` changent) |
| PUT 🛠 | `/api/installations/{id}/cloud` | `{cloud_id: string\|null}` : installation du compte Airzone à laquelle elle correspond (refusé si la MAC du WebServer prouve le contraire) → `Installation` |
| DELETE 🛠 | `/api/installations/{id}` | Supprime (historique et réglages Breezone compris) |
| POST 🛠 | `/api/installations/test` | `{host, port}` → `{ok, zones, webserver}` ou `{ok:false, error}` |
| POST 👑 | `/api/discovery/scan` | `{subnet?}` → `{subnet, found:[{host, port, mac, firmware, zones}]}` (seules les réponses portant la signature Airzone comptent) |
| GET 🔒 / PUT 👑 | `/api/discovery/settings` | Réseau local du serveur : `{subnet}` → `{subnet, effective, in_container}` |
| PATCH 🔒 | `/api/installations/{id}/zones/{system_id}/{zone_id}` | `ZoneCommand` → `Zone` mise à jour ; un champ inconnu → `422 unsupported` |
| PUT 🔒 | `/api/installations/{id}/zones/{system_id}/{zone_id}/meta` | `{display_name?, group?, position?}` |
| GET 🔒 | `/api/installations/{id}/zones/{system_id}/{zone_id}/history?hours=24` | `[{ts, room_temp, humidity, setpoint, is_on, mode, demand_secs, air_quality}]` (`hours` ≤ 2160) |
| GET 🔒 | `/api/installations/{id}/zones/{system_id}/{zone_id}/daily?days=14` | Confort jour par jour : `[DayStats]`, du plus ancien au plus récent (`days` ≤ 90) |
| GET 🔒 | `/api/installations/{id}/trends?hours=24&points=48` | Mini-courbes des cartes : `[{system_id, zone_id, points:[[ts, room_temp]]}]`, température moyenne par tranche (`hours` ≤ 168, `points` de 6 à 200) |
| POST 🔒 | `/api/control/batch` | `{actions:[Action]}` → `{results:[{ok, error}]}` |

`Installation` : `{id, name, location, host, port, enabled, access:"local"|"node", node?:{id, name, state},
controllable, rights:{zones, routines, schedules, admin}, record_demand, connection:{state:"online"|"offline"|"unknown", last_seen, last_error, latency_ms}, zones:[Zone]}`.
`access: "node"` : installation jointe via un autre serveur Breezone (`host` = son adresse) ;
`controllable: false` si ce serveur la partage en lecture seule.

`record_demand` : le temps que chaque zone passe en demande (chaud ou froid) est enregistré avec
l'historique, dans `demand_secs` (secondes en demande depuis le point précédent ; `null` quand il
n'est pas enregistré). Il se règle sur le serveur qui possède l'installation ; les serveurs à qui
elle est partagée suivent ce réglage.

`DayStats` : `{day:"AAAA-MM-JJ", min_temp, max_temp, avg_temp, avg_humidity, recorded_secs, on_secs,
comfort_secs, comfort, demand_secs, air_quality_secs}`, jours locaux du fuseau du serveur (`BREEZONE_TZ`). Chaque point
d'historique vaut jusqu'au suivant (au plus deux intervalles d'historique). `on_secs` : temps allumée
dans un mode qui régule la température (froid, chaud, appoint, auto) ; `comfort_secs` : la part de ce
temps à ±0,5° de la consigne ; `comfort` = `comfort_secs / on_secs` (0 à 1, `null` si la zone n'a pas
été allumée) ; `demand_secs` : `null` si la demande n'était pas enregistrée ; `air_quality_secs` :
`{good, regular, bad}` en secondes, `null` si la zone ne mesure pas la qualité de l'air.

Une installation ajoutée à la main (ou dont l'adresse change) est rattachée automatiquement à
l'installation du compte Airzone qui a le même WebServer (MAC), sans nouvelle connexion à Airzone.

`Zone` : `{installation_id, system_id, zone_id, name, airzone_name, group, position, on, room_temp, humidity,
setpoint, min_temp, max_temp, step, units, mode, modes, is_master, speed, speeds, sleep, double_setpoint,
cool_setpoint, heat_setpoint, demand, air_quality, battery_low, errors}`.

`air_quality` : `null` si le thermostat de la zone ne mesure pas la qualité de l'air (le WebServer
n'envoie alors pas les champs `aq_*`), sinon `{level:"good"|"regular"|"bad"|null, value, mode:"off"|"on"|"auto"|null,
threshold_low, threshold_high}`. `value` est la valeur lue (`aq_quality` : 1 bonne, 2 moyenne,
3 mauvaise, 0 pas de mesure, d'où `level: null`) ; `mode` est celui de la purification de l'air.
L'historique en garde la valeur dans `air_quality` (`null` : pas de capteur). L'endpoint `iaq` du
WebServer n'est jamais appelé : il bloque certains firmwares.

`ZoneCommand` (tous facultatifs) : `{on, setpoint, cool_setpoint, heat_setpoint, mode, speed, sleep}` avec
`mode` ∈ `stop | cooling | heating | fan | dry | aux_heating | auto` (appliqué au système via sa zone
maître) et `sleep` ∈ `0 | 30 | 60 | 90`.

`Action` : `ZoneCommand` + `{installation_id, system_id, zone_id}`.

## Pièces et appareils (ADR 0013)

Les zones Airzone sont aussi des **appareils** du trait `climate.zone@1`, enregistrés automatiquement
avec un identifiant stable. Les routes des zones restent celles qui les pilotent.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/traits` | Catalogue des traits : `[{trait, version, label{fr,en,es}, state:[champ], actions:[{name, label, params:[champ]}]}]`, champ = `{name, type (bool·integer·number·string·enum), unit?, values?, min?, max?, optional, label}`. Sert aussi de descripteur pour un trait inconnu du client |
| GET 🔒 | `/api/devices` | Appareils visibles par l'utilisateur : `[{id, kind, traits, source, source_id, installation_id, reference, name, room_id, role, measures:{device_id, room_id}, position, available, state}]` (`state` : valeurs décrites par le catalogue ; `available` faux si la source ne fournit plus l'appareil) |
| PUT 🔒 | `/api/devices/{id}` | `{name?, room_id?, position?, role?, measures_device_id?, measures_room_id?}` : seuls les champs donnés changent (`null` efface). Nom personnalisé (vide = celui de la source), pièce, et pour une mesure d'énergie son **rôle** : `grid` (réseau, toute la maison), `solar`, `battery` ou `consumption` (consommation d'un appareil ou d'une pièce ; sans cible : autres usages). Même droit que pour renommer les zones de l'installation ; administrateurs pour les appareils des sources |
| GET 🔒 | `/api/rooms` | `[{id, name, position, created_at}]` |
| POST 👑 | `/api/rooms` | `{name, position?}` → 201 |
| PUT 👑 · DELETE 👑 | `/api/rooms/{id}` | Renommer, réordonner, supprimer (ses appareils restent, sans pièce) |

| POST 🔒 | `/api/devices/{id}/actions` | `{trait, action, params}` : action d'un trait de l'appareil, quelle que soit sa source (`climate.zone` / `set` pour une zone Airzone, avec les droits de l'installation ; `switch.relay` / `set` pour un relais Shelly, réservé aux administrateurs pour l'instant). Paramètre inconnu → `422 unsupported` ; manquant ou mal typé → `400` → appareil à jour |
| GET 🔒 | `/api/devices/{id}/history?hours=24&points=96` | `{bucket, series:{field:[[ts, value]]}}` : chaque nombre de l'état d'un appareil d'une source (puissance, énergie, relais en 0/1…), moyenne par intervalle (dernière valeur pour un compteur `energy_in` / `energy_out`). Un point est enregistré toutes les 5 min (`BREEZONE_HISTORY_INTERVAL_SECS`) : moyenne des lectures depuis le point précédent |

### Énergie

Calculée à partir des mesures d'énergie qui ont un rôle (`PUT /api/devices/{id}`), et de leur
historique. Toute valeur est `null` quand la maison n'a pas de mesure de ce rôle.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/energy/live` | `{roles:{grid, solar, battery, consumption}, grid, solar, battery, home, consumers:[{name, device_id, room_id, power}], other}` : puissances (W) à la dernière lecture |
| GET 🔒 | `/api/energy/day?date=YYYY-MM-DD` | `{date, bucket, points:[{ts, grid, solar, battery, home}], totals}` : puissance moyenne par quart d'heure d'un jour local (aujourd'hui par défaut), et son énergie (`totals`, comme un élément de `/days`) |
| GET 🔒 | `/api/energy/days?days=30` | `[{day, imported, exported, production, charged, discharged, consumption, self_consumed, consumers:[{name, device_id, room_id, energy}], other, devices:[{device_id, energy_in, energy_out}]}]` : énergie (kWh) par jour local, du plus ancien à aujourd'hui (1 à 366 jours) |

Conventions :
- `grid` est positif quand la maison tire du réseau. `energy_in` correspond à l'énergie importée, `energy_out` à l'énergie injectée.
- Pour `solar` et `consumption`, seule la valeur absolue compte, car le signe dépend du sens de la pince.
- `battery` est positif en charge.
- La maison consomme : réseau + production + décharge − charge. Sans compteur général, c'est la somme des mesures de consommation.
- `other` est la consommation de la maison qu'aucune mesure de consommation ne couvre.
- L'énergie vient des compteurs des appareils quand ils en ont, sinon de la puissance moyenne.
- Un trou de plus de deux intervalles (appareil non lu) ne compte pas.

### Sources d'appareils 👑

Une **source** est un matériel lu par un **pilote** (ADR 0013) : par exemple un Shelly à
192.168.1.40. Ses appareils apparaissent dans `/api/devices`.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 👑 | `/api/drivers` | Pilotes disponibles : `[{id, label, provides, config:[{name, type, optional, label}]}]` |
| GET 👑 | `/api/sources` | `[{id, driver, name, config, enabled, created_at, status:{state, last_seen, last_error, model, firmware}, devices}]` (`state` : `online`, `offline`, `error`, `disabled`, `unknown`) |
| POST 👑 | `/api/sources` | `{driver, name, config, enabled?}` : configuration vérifiée, source lue aussitôt → 201 |
| PUT 👑 · DELETE 👑 | `/api/sources/{id}` | Modifier (le pilote ne change pas) ; supprimer, avec ses appareils |
| POST 👑 | `/api/sources/{id}/refresh` | Relire maintenant |

| POST 👑 | `/api/discovery/products` | `{subnet?}` → `{subnet, duration_ms, products:[{brand, driver, model, host, port, mac, device_id, firmware, note, supported, added}]}` : tous les produits pris en charge du réseau (WebServers Airzone, Shelly…), identifiés jusqu'au modèle, et s'ils sont déjà ajoutés |

Le scanner interroge chaque adresse pour toutes les marques à la fois (128 adresses en parallèle,
environ 3 s pour un /24) : port 3000 pour Airzone, `GET /shelly` sur le port 80 pour Shelly.

Pilote `shelly` (Plus, Pro, Gen3, Gen4, API RPC locale) : `config: {host, device_id?}`. Avec
`device_id` (fourni par le scanner), un autre appareil qui répondrait à cette adresse est refusé.
Les modèles sont reconnus par leur application (`Shelly Pro 3EM`, en triphasé ou en monophasé à
3 voies, `Shelly Plus Plug S`…) ; la 1re génération est détectée mais pas encore prise en charge. Composants lus :
`em:N` (Pro 3EM, `energy.power` + `energy.meter`), `em1:N` (Pro EM), `switch:N` (`switch.relay`,
avec puissance et énergie s'ils sont mesurés), `pm1:N`. Lecture toutes les 5 s.

Les types web et mobile sont générés depuis `server/contracts/traits/` :
`BREEZONE_WRITE_GENERATED=1 cargo test --test generated` (dans `server/`).

## Routines (scènes, exécutées par Breezone)

Lancées à la demande seulement ; les programmations à heure fixe sont celles d'Airzone (voir
plus bas). Une action peut attendre (`delay_minutes`) : elle devient une minuterie.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/routines` | `[Routine]` |
| POST 🔒 | `/api/routines` | `RoutineInput` |
| GET / PUT / DELETE 🔒 | `/api/routines/{id}` | |
| POST 🔒 | `/api/routines/{id}/run` | Exécute maintenant → `{results}` |

`RoutineInput` : `{name, enabled?, actions:[Action]}` (un `time` est refusé).
`Routine` ajoute `{id, last_run_at, created_at, updated_at}`.

## Compte Airzone (import, lecture seule)

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/cloud` | `{auto_sync, email (admins), last_sync, last_error, installations:[{cloud_id, name, location, zones, installation_id, reachable}]}` |
| PUT 👑 | `/api/cloud` | `{email, password, remember?=true}` → importe maintenant → `{report:{created, updated, unreachable}, status}` |
| POST 👑 | `/api/cloud/sync` | Resynchronise avec les identifiants mémorisés (n'écrase pas les personnalisations) |
| DELETE 👑 | `/api/cloud` | Oublie le compte |
| PUT 👑 | `/api/cloud/installations/{cloud_id}/access` | `{host: string\|null, port?}` : adresse locale fixe du WebServer (vide = recherche réseau) ; vérifie la MAC et rattache l'installation → `{result:{attached, installation_id, message, report}, status}` |

Le rapport de synchronisation contient `notes` : pourquoi une installation n'a pas été jointe.

## Fédération entre serveurs Breezone

Administration (jeton utilisateur) :

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 / PUT 👑 | `/api/node` | Ce serveur : `{node_id, name, public_url}` ; PUT `{name, public_url}` |
| GET 👑 | `/api/shares` | Partages exposés : `[{id, label, permission, installations:[{id,name}], status, pairing_expires_at, pairing_expired, requested_at, reciprocal, remote_shares_back, reverse, expires_at, expired, remote_name, last_seen, last_address, connected}]` |
| POST 👑 | `/api/shares` | `{label, installation_ids, permission:"read"\|"control", expires_in_days?, url?, reciprocal?}` → `{share, token}` (jeton à usage unique, valable 15 minutes, affiché une seule fois) |
| POST 👑 | `/api/shares/{id}/approve` | Accepte la demande d'accès d'un partage `pending` → `active` |
| PUT 👑 | `/api/shares/{id}` | Idem sans `url` : change installations, droit, validité (même clé) |
| DELETE 👑 | `/api/shares/{id}` | Révoque (ou refuse une demande) : le nœud perd l'accès immédiatement |
| GET 🔒 | `/api/nodes` | Nœuds consommés : `[{id, name, url, status:{state, last_seen, last_error, latency_ms, permission}, installations, offered:[{remote_id, name, location, included}], certificate_pinned}]` |
| POST 👑 | `/api/nodes` | `{token, url?, reciprocal?:{installation_ids, permission, url?}, certificate?}` : s'appaire au nœud avec son jeton ; il reste `pending` jusqu'à acceptation. `reciprocal` (jeton réciproque seulement) crée en retour un partage lié à la clé de l'autre nœud |
| PUT 👑 | `/api/nodes/{id}` | `{url}` : change l'adresse du nœud |
| POST 🔒 | `/api/nodes/{id}/refresh` | Interroge le nœud maintenant |
| PUT 👑 | `/api/nodes/{id}/installations` | `{included:[remote_id]}` : installations partagées par ce nœud à afficher ici (les autres sont masquées ; celles qu'il partagera ensuite sont affichées) |
| GET 👑 | `/api/nodes/{id}/certificate` | Certificat HTTPS présenté : `{pinned, certificate:{fingerprint, subject, names, issuer, not_before, not_after, self_signed}}` |
| PUT 👑 | `/api/nodes/{id}/certificate` | `{fingerprint}` : fait confiance à ce certificat (doit être celui présenté, sinon 409) ; seul lui est ensuite accepté |
| DELETE 👑 | `/api/nodes/{id}/certificate` | Rétablit la vérification HTTPS habituelle |
| DELETE 👑 | `/api/nodes/{id}` | Retire le nœud et ses installations |

`state` d'un nœud : `online | pending | certificate | offline | revoked | expired | error | unknown`.

`status` d'un partage : `invited` (jeton pas encore utilisé) → `pending` (demande d'accès reçue) → `active`.

Jeton d'invitation : `bz2.<base64url(JSON {v:2, node_id, name, url, code, reciprocal?})>`. Le code est à usage unique
et expire après 15 minutes ; il ne donne aucun accès par lui-même. Les jetons `bz1.` (ancienne version)
sont refusés. HTTP n'est accepté que vers une adresse locale (IP privée, `*.local`, nom sans point) ;
sinon `https://`.

Certificat refusé par la vérification HTTPS habituelle : `POST /api/nodes` répond 502 avec, en plus
de `error`, `certificate` (comme ci-dessus) et `certificate_error` ; renvoyez la requête avec
`certificate: <empreinte>` pour lui faire confiance (épinglage : seul ce certificat est accepté).

Chaque serveur possède une clé Ed25519 (stockée chiffrée). Toute requête entre serveurs est signée :

| En-tête | Contenu |
|---------|---------|
| `X-Breezone-Node` | `node_id` du serveur appelant |
| `X-Breezone-Timestamp` | Heure Unix (secondes), tolérance ±300 s |
| `X-Breezone-Nonce` | Valeur aléatoire, refusée si déjà vue (rejeu) |
| `X-Breezone-Signature` | Signature Ed25519 (base64url) de `METHODE\nchemin\ntimestamp\nnonce\nsha256hex(corps)` |

Routes entre serveurs — limitées aux installations du partage :

| Méthode | Chemin | Description |
|---------|--------|-------------|
| POST | `/api/federation/pair` | `{code, node_id, name, public_key, shares_back_from?}` signé avec la clé présentée → `{status:"pending", node_id, name, public_key}` ; le code est consommé. `shares_back_from` (adresse du demandeur) n'est retenu que si l'invitation est réciproque : à l'acceptation, ce nœud ajoute le demandeur comme nœud |
| POST | `/api/federation/hello` | Capacités du nœud qui lit (même format que `/api/capabilities`) → capacités de ce nœud. Appelé à l'ouverture du lien et dès que le `caps_rev` de l'un ou de l'autre change. `404` : nœud de 0.10.0 ou avant, traité avec le profil « legacy » |
| GET | `/api/federation/installations?wait=&since=` | `FedSnapshot` : `{node_id, name, permission, installations:[{id, name, location, mac, connection, zones (format Airzone), meta}], revision, caps_rev}`. Avec `since` (la `revision` déjà reçue) et `wait` (secondes, 45 au plus), la réponse attend qu'une installation partagée change ; sans changement à la fin de l'attente : `304`, sans contenu |
| PATCH | `/api/federation/installations/{id}/zones/{system_id}/{zone_id}` | `ZoneCommand` (droit `control`), en-tête `X-Breezone-Actor` pour le journal → installation à jour ; un champ inconnu → `422 unsupported` |
| POST | `/api/federation/installations/{id}/schedules` | `{op:"create", input}` \| `{op:"update", schedule_id, input}` \| `{op:"enable", schedule_id, enabled}` \| `{op:"delete", schedule_id}` : modifie les programmations Airzone de l'installation partagée avec le compte Airzone de ce serveur (partage en pilotage). Un serveur qui joint une installation via un autre nœud lui transmet ainsi ces modifications : son propre compte Airzone peut ne pas avoir le droit de les écrire (abonnement) |

Le serveur consommateur suit chaque nœud en direct : il redemande l'état « depuis la dernière
révision » dès qu'il reçoit une réponse (longue attente, comme un GitLab Runner). Au repos, un
échange de quelques centaines d'octets toutes les 45 s ; l'état complet ne circule qu'à chaque
changement. Si le nœud ne répond pas, nouvel essai après 10 s, puis 1, 2 et 4 min, puis toutes les 5 min jusqu'à son retour ;
au démarrage, chaque serveur rouvre le lien vers tous ses nœuds. Un nœud qui ne connaît pas
l'attente (jusqu'à 0.5.x, sans `revision`) est lu au rythme de lecture des WebServers. Le
consommateur garde sa propre copie de l'historique. Tant que le partage n'est pas accepté, ces routes répondent
403 ; une signature invalide, un rejeu ou un partage révoqué donnent 401.

## Minuteries

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/timers` | `[{id, label, run_at, target, command, created_by, created_at}]` |
| POST 🔒 | `/api/timers` | `{delay_minutes (1 … 10080), target, command: ZoneCommand, label?}` |
| DELETE 🔒 | `/api/timers/{id}` | Annule |
| GET 🔒 | `/api/airzone/routines` | Routines du compte Airzone lues à la synchronisation (`{cloud_id, installation, id, name, enabled, raw}`) |

`target` : `{kind:"zone", installation_id, system_id, zone_id}` | `{kind:"group", installation_id, group}` |
`{kind:"installation", installation_id}` | `{kind:"all"}`, résolue au déclenchement. Une action de
routine peut avoir `delay_minutes` : lancée avec la routine, elle devient une minuterie.

## Programmations Airzone (miroir d'Airzone Cloud)

Airzone reste maître de ses programmations et les exécute ; Breezone les affiche et les modifie
sur Airzone Cloud (l'API locale du WebServer n'en a pas). Nécessite le compte Airzone mémorisé.
Synchronisation toutes les heures, à la demande et après la liaison d'une installation.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/airzone/schedules` | `{last_sync, last_error, account, schedules:[AirzoneSchedule]}` |
| POST 🔒 | `/api/airzone/schedules/sync` | Relit tout sur Airzone Cloud → idem |
| POST 🔒 | `/api/airzone/schedules` | `{cloud_id, name, enabled?, days, time, zones:[{system_id, zone_id}], action}` → crée chez Airzone |
| PUT 🔒 | `/api/airzone/schedules/{cloud_id}/{id}` | Idem sans `cloud_id` → modifie chez Airzone (champs inconnus conservés) |
| PUT 🔒 | `/api/airzone/schedules/{cloud_id}/{id}/enabled` | `{enabled}` |
| DELETE 🔒 | `/api/airzone/schedules/{cloud_id}/{id}` | Supprime chez Airzone |

`AirzoneSchedule` : `{cloud_id, installation, installation_id, id, name, enabled, kind, days (ISO,
1 = lundi), time, zones, action: ZoneCommand ({on:false} | {on:true, setpoint?} | {mode, setpoint?}),
end, editable, note}`. Appels Airzone utilisés (comme l'application officielle, sur
`prodctrl.airzonecloud.com`) : `GET|POST installations/{id}/schedules`,
`PATCH|DELETE installations/{id}/schedule/{schedule id}` ; jours Airzone 0 = lundi … 6 = dimanche.

## Mise à jour de Breezone 👑

Issue #40. Les versions viennent du dépôt officiel seulement, signées par la clé du projet ; une
version qui ne démarre pas est annulée au démarrage suivant (base restaurée, version précédente).

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET | `/api/system/update` | `UpdateView` (le serveur cherche une version une fois par jour) |
| POST | `/api/system/update/check` | Cherche une version maintenant → `UpdateView` (502 si GitHub ne répond pas) |
| POST | `/api/system/update` | Installe la dernière version puis redémarre dessus → 202 `UpdateView` ; 409 s'il n'y a rien à installer ou si une installation est en cours |
| PUT | `/api/system/update/channel` | `{beta}` : rejoint ou quitte le canal bêta → `UpdateView` (la version trouvée sur l'autre canal est oubliée : relancer `check`) |

Canal bêta : les préversions (`x.y.z-alpha.n`, `-beta.n`, `-rc.n`) sont des pré-releases GitHub,
ignorées par `/releases/latest`. Un serveur inscrit au canal bêta lit toutes les releases et prend
la plus récente, préversions comprises (ordre semver : `1.0.0-beta.2 < 1.0.0`). Quitter le canal ne
fait pas revenir en arrière : le serveur reste sur sa bêta jusqu'à une version plus récente.

`UpdateView` : `{current, latest, notes, published_at, url, available, can_install, reason, state,
progress, error, checked_at, rolled_back, beta, prerelease}` (`beta` : inscrit au canal bêta ;
`prerelease` : `latest` est une préversion). `reason` (pourquoi `can_install` est faux) :
`disabled` (`BREEZONE_SELF_UPDATE=false`), `platform` (pas de version pour ce système),
`development` (build de développement), `assets` (fichiers pas encore publiés), `failed` (cette
version n'a pas démarré ici). `state` : `idle`, `downloading` (`progress` de 0 à 1), `installing`,
`restarting`, `failed` (`error`). Pendant `restarting`, le serveur s'arrête quelques secondes puis
revient avec sa nouvelle version (`GET /api/version`). `rolled_back` : `{version, to}` si la dernière
mise à jour n'a pas démarré et a été annulée.

Le binaire répond à `breezone-server --version` (« Breezone x.y.z ») sans démarrer le serveur.

## Journal

`GET 🔒 /api/events?limit=100&before=<id>&installation_id=` → `[{id, ts, kind, level, installation_id, actor, message, data}]`
avec `kind` ∈ `connection | command | routine | system | auth` et `level` ∈ `info | warn | error`.

## Temps réel (WebSocket)

`GET /api/ws`, puis envoyer dans les 10 s : `{"type":"auth","token":"<access_token>","lang":"fr"}` (jamais dans l'URL ; `lang` facultatif, sinon `Accept-Language` de la requête).

Messages reçus :

| `type` | Contenu |
|--------|---------|
| `hello` | `{user}` |
| `snapshot` | `{installations:[Installation]}` (aussi renvoyé après un retard de lecture) |
| `installation` | `{installation}` — à chaque changement |
| `installation_removed` | `{id}` |
| `event` | `{event}` — nouvelle entrée du journal |
| `routines_changed` | — |
| `error` | `{error}` puis fermeture (non authentifié, appareil révoqué) |

Le serveur envoie un ping toutes les 25 s ; le client peut envoyer le texte `ping` (réponse `pong`).
