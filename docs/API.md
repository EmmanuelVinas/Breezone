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
| GET | `/api/users` | Liste, avec `access:[{installation_id, permission:"read"\|"control"\|"admin", rights:{zones, routines, schedules}}]` et `devices:[{scope:"all"\|"room"\|"type"\|"device", ref, permission:"read"\|"control"}]` |
| POST | `/api/users` | `{username, password, role: "admin"\|"user", access?, devices?}` (`rights` d'une entrée : tout autorisé par défaut ; rien n'est donné par défaut) |
| PUT | `/api/users/{id}` | `{role?, password?, access?, devices?}` — un nouveau mot de passe déconnecte l'utilisateur partout ; `access` remplace les installations données et leurs droits, `devices` les appareils des modules donnés |

Les **appareils des modules** (ceux d'aucune installation : Shelly, Hue, volets, VMC…) se donnent à
part, en lecture ou en pilotage : tous (`all`, `ref` vide), ceux d'une pièce (`room`, son id), d'un
type (`type` : `climate`, `charger`, `light`, `shutter`, `window`, `door`, `ventilation`,
`room-air`, `switch`, `solar`, `energy`, `remote`, `other`) ou un appareil (`device`, son id). Le
plus large l'emporte. Un appareil non donné n'apparaît pas dans `/api/devices` et répond 404 ; donné
en lecture, `can_control` est faux et le piloter répond 403. Leurs noms, pièces et rôles restent aux
administrateurs. L'énergie de la maison (`/api/energy/*`, `/api/surplus`) demande les types `energy`
ou `solar` (ou tous les appareils) ; sinon 403. Les comptes d'avant cette version gardent tous les
appareils en lecture seule.
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
| GET 🔒 | `/api/installations` | Configuration (`host`, `port`, `mac`, `location`, `country`, `location_set`, `enabled`, `cloud_id`, `node_id`, `remote_id`, `record_demand`) |
| POST 👑 | `/api/installations` | `{name, host, port?=3000, enabled?=true, record_demand?=false, location?, country?}` : `location` la ville (« Labège, Haute-Garonne »), `country` son code ISO à deux lettres ; une ville saisie n'est plus remplacée par celle du compte Airzone (vide : retour à celle du compte). La météo de la maison s'en sert |
| PUT 🛠 | `/api/installations/{id}` | Idem, `record_demand`, `location` et `country` inchangés s'ils sont absents (installation via un nœud : seuls `name` et `enabled` changent) |
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
| GET 🔒 | `/api/traits` | Catalogue des traits : `[{trait, version, label{fr,en,es}, state:[champ], actions:[{name, label, params:[champ]}]}]`, champ = `{name, type (bool·integer·number·string·enum), unit?, values?, value_labels?, min?, max?, optional, label}` (`value_labels` : `{valeur: label}` pour chaque valeur d'un `enum`). Sert aussi de descripteur pour un trait inconnu du client |
| GET 🔒 | `/api/devices` | Appareils visibles par l'utilisateur : `[{id, kind, traits, source, source_id, installation_id, reference, name, room_id, role, measures:{device_id, room_id}, position, available, state, can_control}]` (`state` : valeurs décrites par le catalogue ; `available` faux si la source ne fournit plus l'appareil ; `can_control` : l'utilisateur peut le piloter — une zone selon ses droits sur l'installation, un appareil de module s'il est administrateur) |
| PUT 🔒 | `/api/devices/{id}` | `{name?, room_id?, position?, role?, measures_device_id?, measures_room_id?}` : seuls les champs donnés changent (`null` efface). `measures_device_id` désigne l'appareil mesuré par une consommation, ou l'onduleur mesuré par une production ; Nom personnalisé (vide = celui de la source), pièce, et pour une mesure d'énergie son **rôle** : `grid` (réseau, toute la maison), `solar`, `battery` ou `consumption` (consommation d'un appareil ou d'une pièce ; sans cible : autres usages). Même droit que pour renommer les zones de l'installation ; administrateurs pour les appareils des sources |
| GET 🔒 | `/api/rooms` | `[{id, name, position, created_at, icon}]` (`icon` : nom Lucide choisi, `null` : deviné d'après le nom par les clients) |
| POST 👑 | `/api/rooms` | `{name, position?, icon?}` → 201 |
| PUT 👑 · DELETE 👑 | `/api/rooms/{id}` | Renommer, réordonner, changer l'icône (`icon` absent : inchangée ; vide : devinée), supprimer (ses appareils restent, sans pièce) |

| POST 🔒 | `/api/devices/{id}/actions` | `{trait, action, params}` : action d'un trait de l'appareil, quelle que soit sa source (`climate.zone` / `set` pour une zone Airzone, avec les droits de l'installation ; `switch.relay` / `set` pour un relais Shelly, réservé aux administrateurs pour l'instant). Paramètre inconnu → `422 unsupported` ; manquant ou mal typé → `400` → appareil à jour |
| GET 🔒 | `/api/devices/{id}/history?hours=24&points=96` ou `?from=&until=&bucket=` | `{bucket, series:{field:[[ts, value]]}}` : chaque nombre de l'état d'un appareil d'une source (puissance, énergie, relais en 0/1…), moyenne par intervalle (dernière valeur pour un compteur `energy_in` / `energy_out`). Avec `from` et `until` (horodatages), sur cette période, par exemple un jour local, et `bucket` secondes par point. Un point est enregistré toutes les 5 min (`BREEZONE_HISTORY_INTERVAL_SECS`) : moyenne des lectures depuis le point précédent |
| GET 🔒 | `/api/devices/{id}/picture` | Image du dernier événement d'une caméra (`camera@1`), déchiffrée : `image/jpeg` ou `image/png` ; `409` sans image (ou chiffrée sans code de vérification) |

### Énergie

Calculée à partir des mesures d'énergie qui ont un rôle (`PUT /api/devices/{id}`), et de leur
historique. Toute valeur est `null` quand la maison n'a pas de mesure de ce rôle.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/energy/live` | `{roles:{grid, solar, battery, consumption}, grid, solar, battery, home, consumers:[{name, device_id, room_id, power}], other}` : puissances (W) à la dernière lecture |
| GET 🔒 | `/api/energy/day?date=YYYY-MM-DD` | `{date, bucket, points:[{ts, grid, solar, battery, home}], totals}` : puissance moyenne par quart d'heure d'un jour local (aujourd'hui par défaut), et son énergie (`totals`, comme un élément de `/days`) |
| GET 🔒 | `/api/energy/days?days=30` | `[{day, imported, exported, production, charged, discharged, consumption, self_consumed, consumers:[{name, device_id, room_id, energy}], other, devices:[{device_id, energy_in, energy_out}]}]` : énergie (kWh) par jour local, du plus ancien à aujourd'hui (1 jour à 5 ans) ; un jour dont le détail est effacé vient de ce qui a été gardé |
| GET 🔒 | `/api/energy/range` | `{first_day}` : premier jour local avec une mesure d'énergie (les périodes proposées s'arrêtent là) |

Conventions :
- `grid` est positif quand la maison tire du réseau. `energy_in` correspond à l'énergie importée, `energy_out` à l'énergie injectée.
- Pour `solar` et `consumption`, seule la valeur absolue compte, car le signe dépend du sens de la pince.
- `battery` est positif en charge.
- La maison consomme : réseau + production + décharge − charge. Sans compteur général, c'est la somme des mesures de consommation.
- `other` est la consommation de la maison qu'aucune mesure de consommation ne couvre.
- Un onduleur solaire (`solar.inverter@1`) est une production sans qu'on le dise. Une mesure de production peut désigner l'onduleur qu'elle mesure (`measures_device_id`, une pince sur sa ligne) : les deux ne comptent qu'une fois, par la mesure. Une mesure liée à un onduleur compte comme une production, quel que soit son rôle. Une mesure de production qui ne dit pas ce qu'elle mesure peut mesurer les onduleurs : ils sont alors laissés de côté, sauf s'ils ont reçu eux-mêmes le rôle `solar` (une autre installation).
- L'énergie vient des compteurs des appareils quand ils en ont, sinon de la puissance moyenne.
- Un trou de plus de deux intervalles (appareil non lu) ne compte pas.

### Prix de l'électricité

Chaque installation (un lieu : la maison, un bureau…) a son contrat d'électricité et ses prix ; il faut
pouvoir la voir pour les lire, l'administrer pour les changer. Ses tarifs sont datés : chacun s'applique à partir d'un jour local (`valid_from`), et changer les prix en
ajoute un, si bien que le passé garde les siens. Les prix sont ceux de la facture, taxes comprises,
par kWh.

Un tarif contient l'abonnement (par mois), le prix de rachat facultatif (`export_price`), des
**profils de jour** et un **calendrier** :
- un profil est découpé en **plages** (`slots`). La première commence à `00:00`, et chacune dure
  jusqu'à la suivante ;
- le calendrier donne le profil de chaque jour de la semaine (`weekdays`, lundi d'abord) ;
- il peut aussi donner un profil par couleur annoncée (`colors`: `blue`, `white`, `red`, pour Tempo),
  qui l'emporte sur le jour de la semaine ;
- `day_start` (Tempo : `06:00`) fait commencer le jour du tarif plus tard que minuit. La nuit garde
  alors le profil de la veille.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/installations/{id}/electricity` | `TariffView` : `{contract, versions:[{valid_from, tariff}], can_edit}` (`contract` vaut `null` tant qu'il n'est pas réglé) |
| PUT 🛠 | `/api/installations/{id}/electricity/contract` | `{country?, supplier?, offer?="custom", power_kva?, currency?="EUR", offpeak?:[{from, to}]}` → `TariffView`. `offer` `base`, `hc` ou `tempo` : les tarifs sont remplacés par ceux de la CRE pour `power_kva` (502 si elle est injoignable ou n'a pas de prix pour cette puissance) ; `offpeak` (heures creuses, à l'heure ou à la demi-heure ; 22:00 → 06:00 par défaut) découpe les prix de l'offre `hc` |
| PUT 🛠 | `/api/installations/{id}/electricity/prices/{YYYY-MM-DD}` | `Tariff` : `{subscription, export_price?, profiles:[{id, name, color?, slots:[{start, name, price}]}], calendar:{day_start?="00:00", weekdays:[7 profils], colors?:{blue?, white?, red?}}}`, le tarif à partir de ce jour (créé ou remplacé) → `TariffView` |
| DELETE 🛠 | `/api/installations/{id}/electricity/prices/{YYYY-MM-DD}` | → `TariffView` ; 404 s'il n'y en a pas |
| GET 🔒 | `/api/installations/{id}/electricity/price?hours=24` | `PriceView` : `{currency, now:{price, slot, profile, profile_name, color, day}, periods:[{start, end, price, slot, profile, profile_name, color}], tempo}`, le prix maintenant et les plages des heures qui viennent (1 h à 7 jours). `now` vaut `null` sans tarif. `tempo` (`null` si aucun tarif ne suit les couleurs) : `{today, tomorrow, red_left, white_left}`, couleurs du jour et du lendemain et jours rouges et blancs restants de la saison (du 1er septembre au 31 août) |

Les offres réglementées d'EDF (`base`, `hc`, `tempo`) prennent les prix publiés en open data par la
CRE (historique du Tarif Bleu résidentiel : `Option_Base.csv`, `Option_HPHC.csv`,
`Option_Tempo.csv`) : un tarif par date de la CRE, aux prix TTC de la puissance souscrite,
l'abonnement annuel divisé par 12, le prix de rachat gardé. Ils sont relus chaque semaine et à chaque
enregistrement du contrat. `BREEZONE_CRE_URL` change l'adresse ; sans accès à internet, ces offres
sont refusées. Une offre `custom` garde les prix saisis.

Les couleurs Tempo viennent du relais public api-couleur-tempo.fr (données de RTE, sans compte). Elles
sont lues toutes les 30 minutes tant qu'un tarif suit les couleurs, avec la saison entière la
première fois. `BREEZONE_TEMPO_URL` change l'adresse ; sans accès à internet (`BREEZONE_WEATHER=0`,
build de développement), rien n'est demandé.

Chaque jour de `/api/energy/day` (`totals`) et de `/api/energy/days` a un `cost`, aux prix de
l'installation `installation_id` (paramètre facultatif ; par défaut la première installation de ce
serveur) (`null` si aucun
tarif ne le couvre) : `{energy, subscription, saved, earned}`. `energy` est l'énergie tirée du réseau,
chaque quart d'heure à son prix ; `subscription` est la part du jour de l'abonnement du mois ;
`saved` est ce qu'aurait coûté au réseau la production utilisée sur place ; `earned` est la revente au
prix de rachat. Sans compteur réseau, l'énergie achetée est la consommation moins la production.

Un tarif est refusé s'il a :
- aucun profil, ou plus de 10 ;
- des plages hors de l'ordre de la journée, ou une première plage qui ne commence pas à `00:00` ;
- un prix hors de 0 à 10 par kWh ;
- un jour du calendrier qui renvoie vers un profil inconnu.

### Historique long

Le détail (toutes les 5 min) est gardé quelques semaines ; chaque heure, le serveur le résume par
heure (gardé environ un an) et par jour local (gardé pour toujours). Une courbe lit le résumé qui
convient à sa période : jusqu'à 5 ans pour `/api/devices/{id}/history` (réponse avec `oldest`, le
plus ancien point) et pour l'historique d'une zone (un point par jour au-delà d'un mois). Les
réglages et valeurs fixes du matériel (`"history": false` dans le catalogue des traits) n'ont pas
d'historique.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/installations/{id}/zones/{system_id}/{zone_id}/history/range` | `{oldest}` : plus ancien point de la zone |

### Météo et soleil

Chaque installation a sa météo. Sa ville est convertie **une fois**, quand elle est réglée (à la
main, depuis le compte Airzone ou par un autre nœud), en coordonnées par le géocodage d'Open-Meteo ;
coordonnées, département et pays sont gardés sur l'installation (jamais envoyés aux clients). La
météo de chaque lieu est relevée toutes les 30 min (un seul relevé pour des installations voisines),
avec la vigilance de son département (MeteoAlarm). Le lever et le coucher du soleil sont calculés
par le serveur. Sans `installation_id` : la maison, c'est-à-dire la première installation de ce
serveur qui a une ville.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/weather?installation_id=` | `{weather:{place, temperature, apparent, humidity, code, precipitation, wind, today_min, today_max, tomorrow_min, tomorrow_max, tomorrow_code, hours:[{ts, temperature, code, precipitation_probability}], read_at, vigilance:{level, events, area} \| null} \| null, sun:{sunrise, sunset} \| null}` (`code` : code météo OMM ; `vigilance.level` : 0 vert … 3 rouge). `online` : faux quand ce serveur ne lit pas la météo (`BREEZONE_WEATHER`, coupée par défaut dans un build de développement) |
| GET 🔒 | `/api/weather/history?hours=24&installation_id=` | `[[ts, °C]]` : température extérieure de l'installation (un point par 30 min, par jour au-delà d'un mois) |
| PUT 👑 | `/api/weather/place` | `{place}` : la ville de la première installation de ce serveur (préférer la ville de chaque installation, `PUT /api/installations/{id}`) |

### Sources d'appareils 👑

`GET /api/sources/{id}/diagnostic` 👑 : ce que le matériel dit de lui-même, brut (base d'un accessoire HomeKit…), avec ce que Breezone en lit ; sans secret.

Une **source** est un matériel lu par un **pilote** (ADR 0013) : par exemple un Shelly à
192.168.1.40. Ses appareils apparaissent dans `/api/devices`.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 👑 | `/api/drivers` | Pilotes disponibles : `[{id, label, provides, config:[{name, type, optional, label}]}]` |
| GET 👑 | `/api/sources` | `[{id, driver, name, config, enabled, created_at, status:{state, last_seen, last_error, model, firmware}, devices}]` (`state` : `online`, `offline`, `error`, `disabled`, `unknown`) |
| POST 👑 | `/api/sources` | `{driver, name, config, enabled?}` : configuration vérifiée, source lue aussitôt → 201 |
| PUT 👑 · DELETE 👑 | `/api/sources/{id}` | Modifier (le pilote ne change pas) ; supprimer, avec ses appareils |
| POST 👑 | `/api/sources/{id}/refresh` | Relire maintenant |
| POST 👑 | `/api/sources/{id}/locate` | Chercher son matériel sur le réseau par son identité (`device_id`) et le suivre s'il a changé d'adresse (« Rechercher ») → la source et `moved` |
| POST 👑 | `/api/homekit/pair` | `{host, name, code, flags?}` : appaire Breezone à un accessoire HomeKit trouvé par le scanner (`host` « ip:port », `flags` ses indicateurs) avec son code à 8 chiffres, puis l'ajoute comme module → 201 la source ; code incorrect, trop d'essais, déjà appairé ailleurs → 422 avec l'explication |
| POST 👑 | `/api/hue/link` | `{host, name}` : ajoute un pont Philips Hue trouvé par le scanner ; il donne une clé d'accès dans les 30 secondes après l'appui sur son bouton → 201 la source ; bouton non pressé → 409 (`code` : `button`) |

| POST 👑 | `/api/discovery/products` | `{subnet?}` → `{subnet, duration_ms, products:[{brand, driver, model, host, port, mac, device_id, firmware, note, supported, added}]}` : tous les produits pris en charge du réseau (WebServers Airzone, Shelly…), identifiés jusqu'au modèle, et s'ils sont déjà ajoutés |

Chaque source retient l'identité de son matériel (`device_id` : id du Shelly, `ID` de la Trydan,
numéro de série de la clé Hoymiles), apprise au premier relevé si elle a été ajoutée par son adresse.
Une source injoignable depuis plus de 3 minutes est cherchée sur le réseau par cette identité (au plus
une fois toutes les 15 minutes) : retrouvée ailleurs, son adresse est mise à jour et le changement est
noté dans le journal d'activité. Le mDNS n'est pas utilisé : sur les réseaux testés, ni le Shelly, ni
la Trydan, ni la clé Hoymiles n'y répondent.

Le scanner interroge chaque adresse pour toutes les marques à la fois (128 adresses en parallèle,
environ 3 s pour un /24) : port 3000 pour Airzone, `GET /shelly` sur le port 80 pour Shelly, `GET /RealTimeData` sur le port 80 pour la Trydan, `AppInfoData` sur le port 10081 pour une clé Hoymiles.

Pilote `shelly` (Plus, Pro, Gen3, Gen4, API RPC locale) : `config: {host, device_id?}`. Avec
`device_id` (fourni par le scanner), un autre appareil qui répondrait à cette adresse est refusé.
Les modèles sont reconnus par leur application (`Shelly Pro 3EM`, en triphasé ou en monophasé à
3 voies, `Shelly Plus Plug S`…) ; la 1re génération est détectée mais pas encore prise en charge. Composants lus :
`em:N` (Pro 3EM, `energy.power` + `energy.meter`), `em1:N` (Pro EM), `switch:N` (`switch.relay`,
avec puissance et énergie s'ils sont mesurés), `pm1:N`. Lecture toutes les 5 s.

Pilote `hue` (pont Philips Hue, API locale version 1 en HTTP) : `config: {host, device_id}` (le
`bridgeid`) ; la clé d'accès, donnée par le pont après l'appui sur son bouton, est le secret de la
source, chiffré, jamais renvoyé. Chaque lampe devient un appareil `light@1` : `on`, `brightness`
(1 à 100 %), `color_temp` (kelvins, avec `color_temp_min` et `color_temp_max`), `color` (« #rrggbb »),
`mode` (`white`, `color`), `reachable` ; seulement ce que la lampe gère. Action `set {on?, brightness?,
color_temp?, color?}` : régler l'intensité, le blanc ou la couleur allume la lampe. Lecture toutes les
5 s. Les lampes, pièces et scènes restent gérées par l'application Hue.

Pilote `homekit` (accessoires et ponts HomeKit en IP : passerelle Legrand Home + Control, VELUX App
Control…) : Breezone est un contrôleur HomeKit. Il s'appaire avec le code de l'accessoire (SRP-6a,
Ed25519), puis lit et pilote l'accessoire en local, par des sessions chiffrées (X25519,
ChaCha20-Poly1305), sans Apple Maison ni compte. Le scanner trouve les accessoires par mDNS
(`_hap._tcp`), en posant la question au groupe et à chaque adresse du réseau. `config: {host, device_id}`
(l'identifiant HomeKit de l'accessoire) ; l'appairage (la clé de Breezone) est le secret de la source,
chiffré, jamais renvoyé par l'API. Une source HomeKit ne s'ajoute que par `/api/homekit/pair`. Volets,
fenêtres et portes deviennent des appareils `cover.position@1` (`position` 0 fermé à 100 ouvert,
`target`, `moving` ; actions `set {position}` et `stop`), prises, interrupteurs et lumières des
`switch.relay@1`. Lecture toutes les 10 s. Appairer Breezone ne change rien d'autre sur l'accessoire ;
supprimer la source retire l'appairage de Breezone, et seulement lui.

Pilote `hoymiles` (micro-onduleurs Hoymiles HM, HMS, HMT, par leur clé Wi-Fi DTU : DTU-W100,
DTU-Lite-S, DTU-WLite, DTU-Pro) : `config: {host, device_id?}` (`device_id` est le numéro de série de la
clé, fourni par le scanner). Protocole local de la clé sur le port TCP 10081, en lecture seule
(`AppInfoData`, `RealDataNew`) : Breezone n'envoie aucune commande. Lue toutes les 30 s seulement, avec
2 s au moins entre deux requêtes, car ces clés supportent mal les interrogations fréquentes. Chaque
micro-onduleur est un appareil `energy.power@1` + `solar.inverter@1` (puissance AC, production du jour
et totale, température, fréquence) qui peut être la production solaire de la page Énergie ; chacun de
ses panneaux est un appareil `solar.panel@1` (puissance, tension, intensité, production). Une clé qui
chiffre ses échanges (firmware récent) est reconnue mais pas encore prise en charge.

Pilote `trydan` (borne V2C Trydan, API HTTP locale sans mot de passe : une adresse fixe est conseillée) :
`config: {host, device_id?}` (`device_id` est l'`ID` de la borne, fourni par le scanner). La borne est un
appareil `ev.charger@1` + `energy.power@1` : elle peut donc être une consommation de la page Énergie.
Actions `ev.charger` :

| Action | Paramètres | Effet sur la Trydan |
|---|---|---|
| `start` | `{limit?, energy?}` | `limit` (6 à 32 A) : Breezone fixe l'intensité maximale et coupe le mode dynamique de la borne (`Dynamic=0`, `Intensity`) ; `energy` (kWh à ajouter) : Breezone arrête la charge une fois cette énergie ajoutée (`energy_goal` dans l'état ; gardé en mémoire, oublié au redémarrage, terminé quand la voiture est débranchée) ; puis `Paused=0` |
| `stop` | `{}` | `Paused=1` (efface l'objectif d'énergie) |
| `set_limit` | `{limit}` | `Intensity` |
| `lock` | `{locked}` | `Locked` |
| `set_auto_limit` | `{on}` | `Dynamic` : la borne règle elle-même l'intensité selon sa pince |

État : `status` (`available`, `plugged`, `charging`, `fault`), `vehicle` (véhicule branché ; absent pendant un défaut), `paused`, `locked`, `session_energy`,
`session_secs`, `limit`, `limit_min`, `limit_max`, `auto_limit`, `energy_goal`, et `power`, `voltage`,
`current` (`energy.power`).

Les types web et mobile sont générés depuis `server/contracts/traits/` :
`BREEZONE_WRITE_GENERATED=1 cargo test --test generated` (dans `server/`).

Les vues principales (`Installation`, `Zone`, `DeviceView`, `Room`…) sont décrites dans
`server/contracts/views.json` : `cargo test --test views` vérifie les vraies réponses du serveur
contre ce contrat, et en écrit les types web (`web/src/lib/api.generated.ts`) et un exemple de
chaque vue que les modèles de l'appli doivent savoir lire (`BREEZONE_WRITE_GENERATED=1`).

## Vue d'ensemble (tableau de bord)

La vue d'ensemble d'une installation est une liste de blocs. Un administrateur de l'installation
fixe celle que tout le monde voit ; chaque utilisateur peut s'en faire une, pour lui seul. Sans
l'une ni l'autre, les clients en composent une à partir des pièces et des appareils.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/installations/{id}/dashboard` | `{layout, source, has_default, can_set_default}` : `source` ∈ `mine` (la sienne) \| `installation` (celle de l'installation) \| `generated` (`layout` nul) |
| PUT 🔒 | `/api/installations/{id}/dashboard` | Enregistre la sienne (`Layout`) |
| DELETE 🔒 | `/api/installations/{id}/dashboard` | Supprime la sienne : retour à celle de l'installation |
| PUT 🔒 | `/api/installations/{id}/dashboard/default` | Celle de l'installation (administrateur de l'installation) ; la sienne est alors effacée |
| DELETE 🔒 | `/api/installations/{id}/dashboard/default` | Supprime celle de l'installation |

`Layout` : `{version: 1, blocks: [{id, type, size, config}]}`, au plus 60 blocs. `size` ∈
`quarter | half | full` (part d'une ligne sur un grand écran). `type` et `config` :
`energy` `{flow: solar|home|battery|grid}` · `surplus` · `group` `{room_id}` (une pièce, `null` :
sans pièce) ou `{type}` (un type d'appareil), avec `hidden` (appareils masqués) · `device`
`{device_id}` · `chart` `{device_id, field, hours: 24|168}` · `weather` · `scenes`
`{routine_ids?}` (aucune : toutes). Un client ignore un type qu'il ne connaît pas.

`?page=` choisit la page (vue d'ensemble par défaut) : `overview`, `energy`, `climate`, `lights`,
`ventilation`, `charging`, `shutters`, et `mobile`, l'accueil de l'app, que chacun arrange dans l'app,
à part de la vue d'ensemble du web. L'app y ajoute ses propres types : `pages` (raccourcis des pages),
`presence`, `zones` `{group?}` (cartes des zones, toutes sans `group`), `energy_flow`,
`energy_curve`, `energy_breakdown` (les parties de son écran Énergie), et `chart`
`{device_id, fields, hours}`.

Les parties des autres pages se posent aussi sur un tableau de bord (web et app) : `zone_history`
et `air_quality` `{device_id}` (une zone), `light_scenes` (scènes Hue), `charger` et `charger_curve`
`{device_id?}` (la première borne sans `device_id`), `solar_today`, `solar_days`, `solar_panels`,
`ventilation_house`. `scenes` peut porter `routine_ids`, et `group` `hidden` (les appareils de la
pièce ou du type laissés hors de la carte).

`GET /api/devices/charted` 🔒 : `[{device_id, fields}]`, les appareils qui ont un historique et ses
champs (ce qu'un graphique peut montrer). L'historique d'une zone Airzone se lit aussi par
`/api/devices/{id}/history` (`room_temp`, `humidity`, `setpoint`). Un bloc `group` peut avoir
`charts` `{device_id: field}` : la courbe de la ligne d'un appareil.

## Routines (scènes, exécutées par Breezone)

Lancées à la demande seulement ; les programmations à heure fixe sont celles d'Airzone (voir
plus bas). Une action peut attendre (`delay_minutes`) : elle devient une minuterie.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/routines` | `[Routine]` |
| POST 🔒 | `/api/routines` | `RoutineInput` |
| GET / PUT / DELETE 🔒 | `/api/routines/{id}` | |
| POST 🔒 | `/api/routines/{id}/run` | Exécute maintenant → `{results}` |

`RoutineInput` : `{name, enabled?, actions:[RoutineAction]}` (un `time` est refusé).
`Routine` ajoute `{id, last_run_at, created_at, updated_at}`.

`RoutineAction` : une commande de zone `{installation_id, system_id, zone_id, …ZoneCommand,
delay_minutes?}`, ou une action d'appareil de n'importe quel module `{device_id, trait, action,
params, delay_minutes?}` (comme `POST /api/devices/{id}/actions`, vérifiée contre le catalogue à
l'enregistrement : 400 / 422 sinon). Droits : une zone demande le pilotage de son installation (et
le droit « routines » pour modifier la scène) ; un appareil de module, un administrateur. Les
scènes qui pilotent des appareils de modules sont visibles de tous, comme `/api/devices`.

## Pilotage sur surplus solaire (fonction `surplus@1`)

Ce qui repartirait au réseau fait tourner des appareils de la maison, par ordre de priorité : une
borne (`ev.charger`, intensité réglée entre un minimum et un maximum) ou une prise pilotée
(`switch.relay`, chauffe-eau, pompe… allumée quand sa puissance tient dans le surplus). Il faut un
compteur avec le rôle `grid`. Seuls les appareils des modules de ce serveur peuvent suivre le
surplus (le surplus est celui de ce site). Le serveur regarde toutes les 10 s.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/surplus` | `{meter, export, loads:[Load], candidates:[{device_id, name, kind}]}` |
| PUT 👑 | `/api/surplus/loads/{device_id}` | `{enabled?, min_amps?, max_amps?, power?, guarantee_kwh?, guarantee_by?, min_on_minutes?, tolerance?}` : ajoute l'appareil (en dernier) ou change ses réglages → la vue |
| DELETE 👑 | `/api/surplus/loads/{device_id}` | Il ne suit plus le surplus (laissé tel quel) |
| PUT 👑 | `/api/surplus/order` | `{device_ids}` : les priorités, la première d'abord → la vue |

`meter` : la maison a un compteur réseau (`null` : pas encore regardé) ; `export` : ce qui part au
réseau (W, négatif : tiré). `Load` : `{device_id, name, kind: "charger"|"relay", position, enabled,
min_amps, max_amps, power, guarantee_kwh, guarantee_by, min_on_minutes, tolerance, limit_min,
limit_max, status: {state, amps, power, text} | null}` ; `state` ∈ `solar` (tourne sur le surplus),
`guarantee` (tourne sur le réseau pour tenir la garantie), `waiting`, `no_car`, `unavailable`,
`no_meter`.

Régulation :
- budget = ce qui part au réseau + ce que tirent déjà les appareils pilotés (une prise qui ne mesure
  rien compte pour sa puissance) ; il est donné aux appareils dans l'ordre ;
- un appareil démarre après 1 min de surplus suffisant, s'arrête après 3 min de manque (une borne
  reste à son minimum entre-temps), ne redémarre pas moins de 2 min après un arrêt ; l'intensité
  d'une borne change au plus toutes les 20 s ; une prise reste allumée `min_on_minutes` au moins ;
  `tolerance` : ce qu'il peut tirer du réseau avant que Breezone ne réduise ;
- garantie d'une borne (`guarantee_kwh` avant `guarantee_by`, « HH:MM ») : quand le soleil ne peut
  plus suffire, elle charge à son maximum depuis le réseau, juste à temps (marge de 15 min) ;
- chaque démarrage et chaque arrêt est dans le journal (`kind: "automation"`).

## Automatismes « quand… si… alors… sinon… » (exécutés par Breezone)

Fonction `automations@1`. Une règle : 1 à 3 déclencheurs (un seul suffit), jusqu'à 8 conditions
(« et » ou « ou », avec des groupes), 1 à 8 étapes « alors » et 0 à 8 étapes « sinon ».

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/automations` | `[Automation]` (celles dont l'utilisateur voit toutes les installations) |
| POST 🔒 | `/api/automations` | `AutomationInput` |
| GET / PUT / DELETE 🔒 | `/api/automations/{id}` | Modifier ou supprimer arrête une exécution en cours |
| PUT 🔒 | `/api/automations/{id}/enabled` | `{enabled}` ; désactiver arrête une exécution en cours |
| POST 🔒 | `/api/automations/{id}/run` | Lance les étapes « alors » maintenant, sans regarder déclencheurs ni conditions → `{results, running}` (`running` : elle attend encore après 10 s ; `409` si elle tourne déjà en mode `single`) |
| GET 🔒 | `/api/automations/{id}/runs` | Les 20 derniers passages : `[{started_at, finished_at, branch, reason, ended, steps:[{what, ok, error}]}]` |
| GET 🔒 | `/api/automations/settings` | `{paused_until, ntfy_url?}` : pause vacances (`null` : pas de pause ; `0` : jusqu'à la reprise ; sinon un instant) ; `ntfy_url` aux administrateurs seulement |
| GET 👑 | `/api/automations/unreadable` | `[{id, name, enabled, updated_at}]` : les règles écrites par une version plus récente, que celle-ci ne sait pas lire (elles ne s'exécutent pas ; un administrateur peut les supprimer par `DELETE /api/automations/{id}`) |
| PUT 👑 | `/api/automations/settings` | `{paused_until?, ntfy_url?}` (seuls les champs donnés changent ; `null` efface) |
| POST 👑 | `/api/automations/notify-test` | Envoie une notification de test (journal et ntfy) |
| GET 🔒 | `/api/presence` | `{me, someone_home, people:[{user_id, username, home, changed_at}]}` (fonction `presence@1`) |
| PUT 🔒 | `/api/presence` | `{home}` : l'utilisateur dit s'il est à la maison |

`AutomationInput` : `{name, enabled?, triggers: [Trigger], match?: "all"|"any", conditions?: [Condition], actions: [Step], else?: [Step], mode?: "single"|"restart", over?: [Step]}`.
`over` (« quand ce n'est plus vrai ») : les étapes lancées quand le déclencheur qui dure (mesure, état,
façade, présence) et qui a démarré « alors » ne tient plus — repassé de l'autre côté du seuil avec la
marge, aussi longtemps que `for_minutes`. Les étapes de « alors » encore en cours s'arrêtent alors ;
`restore` y remet ce que « alors » a changé. Refusé (`400`) sans déclencheur qui dure ; rien pour
« Essayer » ni pour « sinon ».
`when: Trigger` (un seul déclencheur, forme de 0.11.0-beta.18) est encore accepté à la place de `triggers`.

`Trigger` :
- `{kind:"measure", measure: Measure, op:"above"|"below", value, for_minutes? (0 … 1440)}` ;
- `{kind:"state", device_id, field, value, for_minutes?}` : un oui/non ou un choix de l'état d'un
  appareil **devient** `value` (« la borne commence à charger ») ;
- `{kind:"time", time:"HH:MM", days?:[1 … 7]}` (1 = lundi ; vide : tous les jours) ;
- `{kind:"sun", event:"sunrise"|"sunset"|"dawn"|"dusk", offset_minutes? (−180 … 180), days?}`
  (aube et crépuscule civils : le soleil à 6° sous l'horizon) ;
- `{kind:"facade", facing}` : le soleil arrive sur une façade orientée à `facing` degrés (0 nord,
  180 sud) : à plus de 3° de hauteur et à moins de 80° de son orientation ;
- `{kind:"button", device_id, button?, press?: "single"|"double"|"long"}` : un bouton d'une
  télécommande (`button@1`) ; tout bouton, tout appui quand ils manquent ;
- `{kind:"presence", home}` : le premier arrive à la maison (`true`) ou le dernier part (`false`).

`Measure` : `{kind:"energy", flow:"export"|"import"|"solar"|"home"}` (W ; `export` = puissance
injectée au réseau) | `{kind:"device", device_id, field}` | `{kind:"outdoor"}` |
`{kind:"weather", field:"precipitation"|"wind"|"rain_next_hour"|"humidity"|"tomorrow_min"|"tomorrow_max"|"vigilance"}`
(mm, km/h, %, %, °C ; relevé Open-Meteo de moins de 3 h ; `humidity` : humidité extérieure) |
`{kind:"price"}` (le prix de l'électricité maintenant, par kWh, d'après le tarif ; aucune valeur sans
tarif) | `{kind:"tempo", day:"today"|"tomorrow"}` (couleur Tempo : 1 bleu, 2 blanc, 3 rouge ; aucune
valeur tant qu'elle n'est pas annoncée : « demain rouge » = `tomorrow` `above` 2). La
météo, la température extérieure et le soleil d'une règle sont ceux de l'installation de la première
zone qu'elle pilote, sinon qu'elle lit ; à défaut, ceux de la maison. `vigilance` : l'avertissement en cours pour
le département de la maison, 0 vert, 1 jaune, 2 orange, 3 rouge (vigilance Météo-France relayée par
MeteoAlarm, flux public sous licence CC BY 4.0 ; France, Belgique, Suisse, Espagne, Italie,
Allemagne, Portugal, Luxembourg, Pays-Bas, Autriche).

`Condition` : `{kind:"measure", measure, op, value}` | `{kind:"state", device_id, field, value}` |
`{kind:"months", from, to}` (1 … 12 ; `from` > `to` passe par le nouvel an) | `{kind:"hours",
from:"HH:MM", to:"HH:MM"}` | `{kind:"days", days}` | `{kind:"daylight", day}` | `{kind:"facade",
facing}` | `{kind:"presence", home}` (quelqu'un est à la maison, ou personne) |
`{kind:"group", match, conditions}` (2 conditions ou plus, un seul niveau). `match` dit
comment les conditions sont réunies : `all` (et, par défaut) ou `any` (ou) ; une condition qu'on ne
peut pas lire (appareil injoignable) ne décide que si les autres ne le font pas.

`Step` : `{kind:"device", device_id, trait, action, params}` (comme `POST /api/devices/{id}/actions`) |
`{kind:"group", device_type, room_id?, trait, action, params}` (la même action sur chaque appareil
des modules de ce type — voir `devices::TYPES` — dans la pièce, ou partout sans `room_id` ; les
appareils sont cherchés à chaque exécution, un appareil ajouté plus tard suit ; refusé sans appareil ;
pour un utilisateur, il faut le pilotage de tout le type ou de toute la pièce, pas appareil par
appareil ; un appareil en échec n'empêche pas les autres) |
`{kind:"scene", routine_id}` | `{kind:"wait", seconds}` (1 … 86400) | `{kind:"wait_until",
condition, for_seconds?, timeout_seconds, on_timeout?: "continue"|"stop"}` (attend que la condition
soit vraie, `for_seconds` d'affilée, au plus `timeout_seconds`) | `{kind:"restore"}` (remet les
appareils pilotés plus haut dans la même exécution comme ils étaient : réglages changés, charge,
boost ; vient après une action sur un appareil ; un appareil dont quelqu'un a changé un de ces
réglages entre-temps est laissé tel quel, et l'historique le dit) | `{kind:"notify", message}` (1 à 200 caractères : écrit dans
le journal, `kind: "notification"`, que les applis ouvertes affichent aussitôt ; envoyé aussi au
sujet ntfy réglé, avec le titre « Breezone · nom de la règle »).

`Automation` ajoute `{id, when` (le premier déclencheur)`, summary:{when, triggers, conditions,
joined, actions, else, over}` (la règle en phrases, dans la langue de la requête ; `joined` = « et » ou
« ou »)`, status:{value, unit, holds, conditions:[true|false|null], next_at, running}, last_run_at,
last_reason, conflicts:[{id, name, what}], created_at, updated_at}` (`conflicts` : les règles activées
qui peuvent donner au même appareil un autre réglage au même moment).

`branch` d'un passage : `then`, `else`, `skipped` (déclenchée, conditions fausses, pas de « sinon » :
`reason` dit lesquelles et ce qu'elles valaient) ou `busy` (elle tournait déjà). `ended` : `done`,
`stopped` (une attente a expiré avec `on_timeout: stop`), `cancelled` (recommencée, modifiée ou
désactivée), `interrupted` (arrêt du serveur) ; `null` tant qu'elle tourne.

Comportement :
- un déclencheur qui dure (mesure, état, façade) démarre la règle **une fois par épisode** : quand il
  a tenu `for_minutes` et que les conditions sont vraies (réessayées tant qu'il tient), ou aussitôt
  avec « sinon » s'il y en a. Il se réarme quand il ne tient plus (une mesure : repassée de l'autre
  côté du seuil avec une marge : 10 % et au moins 50 W ; 0,5 °C ; 2 points de % ; 5 km/h). Un état ou
  la façade ne déclenche que sur un changement : déjà vrai au démarrage du serveur, il ne compte pas ;
- au plus un démarrage par règle et par minute ; une heure fixe manquée pendant un arrêt de moins de
  15 min est rattrapée ;
- les étapes s'exécutent en arrière-plan ; redéclenchée pendant qu'elle attend, la règle ignore ce
  nouveau départ (`single`, noté `busy`) ou recommence (`restart`) ; un arrêt du serveur termine les
  exécutions en cours ;
- chaque exécution est dans le journal (`kind: "automation"`) avec sa raison ;
- en pause vacances, aucune règle ne démarre d'elle-même (« Essayer » reste possible) et rien n'est
  rattrapé à la reprise.

Droits : piloter une zone demande le pilotage de son installation et le droit « routines » ; les
appareils des modules (borne, relais, lumières…) sont réservés aux administrateurs.

## Compte Airzone (import, lecture seule)

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/cloud` | `{auto_sync, email (admins), last_sync, last_error, installations:[{cloud_id, name, location, zones, installation_id, reachable}]}` |
| PUT 👑 | `/api/cloud` | `{email, password, remember?=true}` → importe maintenant → `{report:{created, updated, unreachable}, status}` |
| POST 👑 | `/api/cloud/sync` | Resynchronise avec les identifiants mémorisés (n'écrase pas les personnalisations) |
| DELETE 👑 | `/api/cloud` | Oublie le compte |
| PUT 👑 | `/api/cloud/installations/{cloud_id}/access` | `{host: string\|null, port?}` : adresse locale fixe du WebServer (vide = recherche réseau) ; vérifie la MAC et rattache l'installation → `{result:{attached, installation_id, message, report}, status}` |

Le rapport de synchronisation contient `notes` : pourquoi une installation n'a pas été jointe.

## Comptes et services liés 👑

Un compte relie Breezone à un service en ligne, toujours en plus de ce qu'il lit en local. Les
secrets (mot de passe, secret client, jetons) sont chiffrés avec la clé du serveur et ne sont
jamais renvoyés. Types : `airzone` (Airzone Cloud, connecté depuis le module Airzone, voir plus
bas), `rte` (couleur Tempo officielle), `linky` (Conso API, ce qu'Enedis a mesuré), `netatmo`
(Legrand Home + Control), `ezviz` (caméras et sonnettes, compte de l'application EZVIZ), et le service `weather` (Open-Meteo, MeteoAlarm : sans identifiants,
toujours `connected`, `service: true`).

Chacun se désactive sans être oublié : ce qui s'en sert fait comme s'il n'était pas là (Tempo par
le relais public, le module Netatmo arrêté, plus de synchronisation Airzone, plus de météo :
`/api/weather` répond `online: false`).

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 👑 | `/api/accounts` | `[{kind, connected, enabled, service, state, last_error, last_ok, expires_at, config}]`, tous les types ; `state` ∈ `ok \| error \| expired \| unknown \| none` ; `config` : ce qui n'est pas secret (`email`, `client_id`, `prm`, `homes` de Netatmo) |
| PUT 👑 | `/api/accounts/{kind}` | Connecte après vérification auprès du service (`502` s'il refuse, rien n'est gardé) : `rte` `{client_id, client_secret}` ; `linky` `{token, prm}` (lit l'année passée) ; `netatmo` `{client_id, client_secret, refresh_token}` (jeton du « Token generator », droits `read_magellan write_magellan` ; Breezone garde le nouveau refresh token donné à chaque renouvellement) ; `ezviz` `{account, password, code?, image_code?}` (le MD5 du mot de passe est gardé ; un compte à double authentification répond d'abord `400` après avoir fait envoyer un code, à redonner dans `code` ; `image_code` : code de vérification des caméras, pour leurs images chiffrées) |
| POST 👑 | `/api/accounts/{kind}/test` | Utilise le compte une fois de plus → la vue, avec son état et sa dernière erreur |
| PUT 👑 | `/api/accounts/{kind}/enabled` | `{enabled}` : l'active ou le désactive → la vue |
| DELETE 👑 | `/api/accounts/{kind}` | Oublie le compte (`airzone` : comme `DELETE /api/cloud` ; `weather` : `400`, il se désactive) |
| GET 🔒 | `/api/energy/linky?days=30&installation_id=` | `{prm, days: [{day, consumption, production, cost, estimated, measured}]}` (kWh) : `cost` au tarif de l'installation, chaque demi-heure à son prix quand la courbe est connue (`estimated` sinon : le total du jour au prix de midi) ; `measured`, ce que les modules ont mesuré tiré du réseau ce jour-là. Pour ceux qui voient l'énergie |

Avec un compte `netatmo`, le module `netatmo` (« Netatmo · Legrand », cloud) est ajouté seul aux
sources. Il lit les maisons du compte toutes les minutes : volets, lumières, prises, et lignes de
l'Écocompteur (`energy.power@1`). Un appareil qu'une source locale a déjà (même nom, même famille :
volet, marche-arrêt…) n'est pas ajouté une seconde fois : il reste celui de la source locale, et
le cloud ne sert qu'aux actions que le local refuse ou quand il ne répond plus (arrêt d'un volet
Legrand : `target_position: -1`). Un volet local qui a un tel jumeau se dit `stoppable`.
Le rapprochement se fait par les mots du nom (sans les mots qui ne disent que le type : volet,
roulant…), d'abord les noms identiques puis ceux contenus l'un dans l'autre, ou à la main :
`GET /api/sources/{id}/twins` 👑 → `{devices: [{reference, name, kind, status: own|linked|ignored,
chosen, twin_of: [source_id, reference] | null, state?}], locals: [{source_id, reference, name, kind,
source}]}` (`state` : son dernier relevé, pour le reconnaître) ; `PUT /api/sources/{id}/twins` 👑
`{reference, choice: auto|link|own|ignore, source_id?, local?}` → idem. Un appareil relié ou ignoré
n'est plus un appareil (sa ligne est supprimée).

Les mêmes routes servent à tout module, local compris : `status` y vaut `own` ou `ignored`, `locals`
est vide, et seuls `choice: ignore` (le laisser de côté) et `auto` (le rétablir) sont admis. Les
choix sont gardés dans la configuration du module (`ignored`, `links`, `own`) ; un `PUT
/api/sources/{id}` qui ne les donne pas les conserve.

Avec un compte `rte`, la couleur des jours Tempo vient de RTE (la saison une fois, puis aujourd'hui
et demain) ; sans lui, ou si RTE échoue, du relais public. Le compte `linky` est relu toutes les
6 h (les journées arrivent le lendemain). Bloc de tableau de bord : `linky` `{days}`.

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
routine peut avoir `delay_minutes` : lancée avec la routine, elle devient une minuterie ; sur un
appareil, sa cible est `{kind:"device", device_id, trait, action, params}` (et `command` est vide),
visible des administrateurs seulement. On ne crée pas ce type de minuterie par `POST /api/timers`.

## Programmations Airzone (miroir d'Airzone Cloud)

Airzone reste maître de ses programmations et les exécute ; Breezone les affiche et les modifie
sur Airzone Cloud (l'API locale du WebServer n'en a pas). Nécessite le compte Airzone mémorisé.
Synchronisation toutes les heures, à la demande et après la liaison d'une installation.

| Méthode | Chemin | Description |
|---------|--------|-------------|
| GET 🔒 | `/api/airzone/schedules` | `{last_sync, last_error, account, paused, schedules:[AirzoneSchedule]}` : `paused` : Breezone les a toutes désactivées, et aucune n'a été réactivée depuis (ici ou dans l'application Airzone) |
| POST 🔒 | `/api/airzone/schedules/sync` | Relit tout sur Airzone Cloud → idem |
| POST 🔒 | `/api/airzone/schedules` | `{cloud_id, name, enabled?, days, time, zones:[{system_id, zone_id}], action}` → crée chez Airzone |
| PUT 🔒 | `/api/airzone/schedules/{cloud_id}/{id}` | Idem sans `cloud_id` → modifie chez Airzone (champs inconnus conservés) |
| PUT 🔒 | `/api/airzone/schedules/{cloud_id}/{id}/enabled` | `{enabled}` |
| PUT 🔒 | `/api/airzone/schedules/enabled` | `{enabled}` (administrateur) : `false` désactive chez Airzone toutes celles qui sont actives et retient lesquelles, pour que les automatismes et programmations de Breezone ne leur fassent pas concurrence ; `true` réactive celles-là seulement |
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
avec `kind` ∈ `connection | command | routine | automation | notification | presence | system | auth` et `level` ∈ `info | warn | error`.

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
| `routines_changed` | — (scènes, minuteries ou automatismes ont changé) |
| `error` | `{error}` puis fermeture (non authentifié, appareil révoqué) |

Le serveur envoie un ping toutes les 25 s ; le client peut envoyer le texte `ping` (réponse `pong`).
