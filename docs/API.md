# API Breezone

Base : `http(s)://<serveur>` — JSON, champs en `snake_case`. Erreurs : statut HTTP + `{"error": "…"}`.

Authentification : `Authorization: Bearer <access_token>`. Le jeton d'accès dure 15 min ; on en obtient
un nouveau avec le refresh token de l’appareil.
🔒 = authentifié, 👑 = administrateur.

## Premier démarrage et sessions

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET | `/health` | `{"status":"ok","version":"…"}` |
| GET | `/api/setup` | `{"needs_setup": bool}` — vrai tant qu'aucun compte n'existe |
| POST | `/api/setup` | `{username, password, device_name?}` → crée l'administrateur, renvoie un `TokenPair` |
| POST | `/api/auth/login` | `{username, password, device_name?}` → `TokenPair` (verrou 15 min après 5 échecs) |
| POST | `/api/auth/refresh` | `{refresh_token}` → nouveau `TokenPair` ; l'ancien refresh token reste valable 120 s |
| POST | `/api/auth/logout` | `{refresh_token}` → révoque l'appareil |
| GET 🔒 | `/api/me` | Utilisateur courant |
| PUT 🔒 | `/api/me/password` | `{current_password, new_password}` — déconnecte les autres appareils |
| GET 🔒 | `/api/sessions` | Appareils connectés (`current` = celui-ci) |
| DELETE 🔒 | `/api/sessions/{id}` | Déconnecte un appareil (immédiat, WebSocket compris) |

`TokenPair` : `{access_token, refresh_token, token_type, expires_in, refresh_expires_at, user}`.

## Utilisateurs 👑

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET | `/api/users` | Liste |
| POST | `/api/users` | `{username, password, role: "admin"\|"user"}` |
| PUT | `/api/users/{id}` | `{role?, password?}` — un nouveau mot de passe déconnecte l'utilisateur partout |
| DELETE | `/api/users/{id}` | Impossible pour soi-même ou le dernier administrateur |

## Installations et zones

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/status` | État complet : `[Installation]` |
| GET 🔒 | `/api/installations/{id}` | Une `Installation` |
| POST 🔒 | `/api/installations/{id}/refresh` | Interroge immédiatement le WebServer |
| GET 🔒 | `/api/installations` | Configuration (`host`, `port`, `mac`, `location`, `enabled`, `cloud_id`, `node_id`, `remote_id`) |
| POST 👑 | `/api/installations` | `{name, host, port?=3000, enabled?=true}` |
| PUT 👑 | `/api/installations/{id}` | Idem (installation via un nœud : seuls `name` et `enabled` changent) |
| PUT 👑 | `/api/installations/{id}/cloud` | `{cloud_id: string\|null}` : installation du compte Airzone à laquelle elle correspond (refusé si la MAC du WebServer prouve le contraire) → `Installation` |
| DELETE 👑 | `/api/installations/{id}` | Supprime (historique et réglages Breezone compris) |
| POST 👑 | `/api/installations/test` | `{host, port}` → `{ok, zones, webserver}` ou `{ok:false, error}` |
| POST 👑 | `/api/discovery/scan` | `{subnet?}` → `{subnet, found:[{host, port, mac, firmware, zones}]}` (seules les réponses portant la signature Airzone comptent) |
| GET 🔒 / PUT 👑 | `/api/discovery/settings` | Réseau local du serveur : `{subnet}` → `{subnet, effective, in_container}` |
| PATCH 🔒 | `/api/installations/{id}/zones/{system_id}/{zone_id}` | `ZoneCommand` → `Zone` mise à jour |
| PUT 🔒 | `/api/installations/{id}/zones/{system_id}/{zone_id}/meta` | `{display_name?, group?, position?}` |
| GET 🔒 | `/api/installations/{id}/zones/{system_id}/{zone_id}/history?hours=24` | `[{ts, room_temp, humidity, setpoint, is_on, mode}]` |
| POST 🔒 | `/api/control/batch` | `{actions:[Action]}` → `{results:[{ok, error}]}` |

`Installation` : `{id, name, location, host, port, enabled, access:"local"|"node", node?:{id, name, state},
controllable, connection:{state:"online"|"offline"|"unknown", last_seen, last_error, latency_ms}, zones:[Zone]}`.
`access: "node"` : installation jointe via un autre serveur Breezone (`host` = son adresse) ;
`controllable: false` si ce serveur la partage en lecture seule.

Une installation ajoutée à la main (ou dont l'adresse change) est rattachée automatiquement à
l'installation du compte Airzone qui a le même WebServer (MAC), sans nouvelle connexion à Airzone.

`Zone` : `{installation_id, system_id, zone_id, name, airzone_name, group, position, on, room_temp, humidity,
setpoint, min_temp, max_temp, step, units, mode, modes, is_master, speed, speeds, sleep, double_setpoint,
cool_setpoint, heat_setpoint, demand, battery_low, errors}`.

`ZoneCommand` (tous facultatifs) : `{on, setpoint, cool_setpoint, heat_setpoint, mode, speed, sleep}` avec
`mode` ∈ `stop | cooling | heating | fan | dry | aux_heating | auto` (appliqué au système via sa zone
maître) et `sleep` ∈ `0 | 30 | 60 | 90`.

`Action` : `ZoneCommand` + `{installation_id, system_id, zone_id}`.

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
| GET 👑 | `/api/shares` | Partages exposés : `[{id, label, permission, installations:[{id,name}], expires_at, expired, remote_name, last_seen, last_address, connected}]` |
| POST 👑 | `/api/shares` | `{label, installation_ids, permission:"read"\|"control", expires_in_days?, url?}` → `{share, token}` (jeton affiché une seule fois) |
| PUT 👑 | `/api/shares/{id}` | Idem sans `url` : change installations, droit, validité (même clé) |
| DELETE 👑 | `/api/shares/{id}` | Révoque : la clé cesse de fonctionner immédiatement |
| GET 🔒 | `/api/nodes` | Nœuds consommés : `[{id, name, url, status:{state, last_seen, last_error, latency_ms, permission}, installations}]` |
| POST 👑 | `/api/nodes` | `{token, url?}` : ajoute un nœud depuis son jeton, crée ses installations |
| PUT 👑 | `/api/nodes/{id}` | `{url}` : change l'adresse du nœud |
| POST 🔒 | `/api/nodes/{id}/refresh` | Interroge le nœud maintenant |
| DELETE 👑 | `/api/nodes/{id}` | Retire le nœud et ses installations |

`state` d'un nœud : `online | offline | revoked | expired | error | unknown`.

Jeton d'invitation : `bz1.<base64url(JSON {v:1, node_id, name, url, key})>`. HTTP n'est accepté que
vers une adresse locale (IP privée, `*.local`, nom sans point) ; sinon `https://`.

Entre serveurs (`Authorization: Bearer bzk_…`, la clé du partage) — limité aux installations du partage :

| Méthode | Chemin | Description |
|---------|--------|-------------|
| POST | `/api/federation/hello` | `{node_id, name}` du serveur appelant → `FedSnapshot` |
| GET | `/api/federation/installations` | `FedSnapshot` : `{node_id, name, permission, installations:[{id, name, location, mac, connection, zones (format Airzone), meta}]}` |
| PATCH | `/api/federation/installations/{id}/zones/{system_id}/{zone_id}` | `ZoneCommand` (droit `control`), en-tête `X-Breezone-Actor` pour le journal → installation à jour |

Le serveur consommateur interroge chaque nœud à son rythme de lecture (une requête par nœud) et
garde sa propre copie de l'historique.

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

## Journal

`GET 🔒 /api/events?limit=100&before=<id>&installation_id=` → `[{id, ts, kind, level, installation_id, actor, message, data}]`
avec `kind` ∈ `connection | command | routine | system | auth` et `level` ∈ `info | warn | error`.

## Temps réel (WebSocket)

`GET /api/ws`, puis envoyer dans les 10 s : `{"type":"auth","token":"<access_token>"}` (jamais dans l'URL).

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
