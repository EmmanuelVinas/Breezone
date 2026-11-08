# API Breezone

Base : `http(s)://<serveur>` — JSON, champs en `snake_case`. Erreurs : statut HTTP + `{"error": "…"}`.

Authentification : `Authorization: Bearer <access_token>`. Le jeton d'accès dure 15 min ; on en obtient
un nouveau avec le refresh token de l’appareil.
🔒 = authentifié, 👑 = administrateur.

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

## Premier démarrage et sessions

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET | `/health` | `{"status":"ok","version":"…","api_version":1,"min_client_api":1}` |
| GET | `/api/setup` | `{"needs_setup": bool}` — vrai tant qu'aucun compte n'existe |
| POST | `/api/setup` | `{username, password, device_name?}` → crée l'administrateur, renvoie un `TokenPair` |
| POST | `/api/auth/login` | `{username, password, device_name?}` → `TokenPair` (verrou 15 min après 5 échecs) |
| POST | `/api/auth/refresh` | `{refresh_token}` → nouveau `TokenPair` ; l'ancien refresh token reste valable 120 s |
| POST | `/api/auth/logout` | `{refresh_token}` → révoque l'appareil |
| GET 🔒 | `/api/me` | Utilisateur courant, avec `access` : installations visibles (`null` pour un administrateur) |
| PUT 🔒 | `/api/me/password` | `{current_password, new_password}` — déconnecte les autres appareils |
| GET 🔒 | `/api/sessions` | Appareils connectés (`current` = celui-ci) |
| DELETE 🔒 | `/api/sessions/{id}` | Déconnecte un appareil (immédiat, WebSocket compris) |

`TokenPair` : `{access_token, refresh_token, token_type, expires_in, refresh_expires_at, user}`.

## Utilisateurs 👑

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET | `/api/users` | Liste, avec `access:[{installation_id, permission:"read"\|"control"}]` |
| POST | `/api/users` | `{username, password, role: "admin"\|"user", access?}` |
| PUT | `/api/users/{id}` | `{role?, password?, access?}` — un nouveau mot de passe déconnecte l'utilisateur partout ; `access` remplace les installations données |
| DELETE | `/api/users/{id}` | Impossible pour soi-même ou le dernier administrateur |

Un administrateur voit et pilote toutes les installations. Un utilisateur (`role: "user"`) ne voit
que celles de son `access` : les autres sont absentes de toutes les réponses et du WebSocket (404 si
on les demande), une installation en `read` est envoyée avec `controllable: false` et ses commandes
répondent 403. Les minuteries et routines ne peuvent viser que des installations pilotables par
l'utilisateur (la cible « toutes » est réservée aux administrateurs). Une installation ajoutée
ensuite (à la main, depuis le compte Airzone ou via un autre nœud) n'est donnée à personne.

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
| GET | `/api/federation/installations` | `FedSnapshot` : `{node_id, name, permission, installations:[{id, name, location, mac, connection, zones (format Airzone), meta}]}` |
| PATCH | `/api/federation/installations/{id}/zones/{system_id}/{zone_id}` | `ZoneCommand` (droit `control`), en-tête `X-Breezone-Actor` pour le journal → installation à jour |

Le serveur consommateur interroge chaque nœud à son rythme de lecture (une requête par nœud) et
garde sa propre copie de l'historique. Tant que le partage n'est pas accepté, ces routes répondent
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
