# Feuille de route

État au 2026-11-06.

## Fait

- Serveur Rust : API locale Airzone, temps réel, programmations, historique, journal, utilisateurs.
- Import du compte Airzone (installations, ville, groupes), rapprochement par adresse MAC.
- Une seule recherche réseau (celle du serveur), réglable dans l'interface ; adresse locale fixe par
  installation, vérifiée par la MAC.
- Interface web, application Android / iOS multi-serveurs, image Docker, scripts.

## À venir

### 1. Nœuds Breezone fédérés

- [x] API de fédération (état, commandes) authentifiée par clé de partage, limitée au partage
- [x] Réglages › **Partages** : installations exposées, droit, expiration, jeton + QR code, révocation,
      « à qui j'expose mon nœud » (dernière connexion, adresse, connecté ou non)
- [x] Réglages › **Nœuds** : ajout par jeton, indicateur de connexion (connecté, injoignable, clé
      révoquée, expirée), changement d'adresse, retrait
- [x] Point d'accès par installation : réseau du serveur (IP du WebServer) **ou** via un autre serveur,
      au choix dans la fenêtre « Accès »
- [x] Programmations multi-sites (les commandes passent par le nœud qui expose)
- [x] Installation ajoutée à la main rattachée au compte Airzone (automatiquement par la MAC, ou au
      choix dans Réglages › Installations) : plus de doublon
- [ ] WebSocket entre nœuds (aujourd'hui : lecture à chaque cycle)
- [x] Application mobile : « via … » sur les installations d'un autre serveur, commandes bloquées si
      partagée en lecture seule
- [ ] Application mobile : scan du QR code pour ajouter un nœud (caméra Android / iOS)

### 2. Accès par utilisateur et par installation

- [ ] Rôles `viewer` / `operator` par installation, filtrage côté serveur (API, WebSocket, journal)
- [ ] Écrans : par utilisateur (ses installations) et par installation (qui y a accès)

### 3. Connexion locale de l'application

- [ ] HTTP autorisé seulement vers les adresses privées (vérifié par l'application)
- [ ] Deux adresses par serveur (locale / distante) avec bascule automatique ; `server_id` dans `/health`
- [ ] Option : `scripts/local-https.sh` (Caddy, certificat Let's Encrypt par défi DNS)

### 4. Graphiques de zone, comme dans l'application Airzone

L'application officielle propose par zone un graphique **« Confort »** et, si la zone a un capteur,
un graphique **« Qualité d'air »**, plus un graphique de **consommation** par installation.
Breezone a déjà une courbe température / consigne / humidité sur la page de chaque zone.

- [ ] Graphique « Confort » au même niveau de détail : navigation jour par jour, périodes de
      demande (chauffe / froid) en bandes, mode, plages de consigne
- [ ] Mini-courbe des dernières heures sur chaque carte de zone (web et mobile)
- [ ] Qualité d'air (PM1, PM2,5, PM10, indice) si la zone a un capteur
- [ ] Consommation : estimation par le taux de demande, ou mesure par compteur

### 5. Application mobile : qualité

- [x] Skills du projet (`.claude/skills/`) : `jetpack-compose-performance` (officiel Google,
      `android/skills`) et `compose-multiplatform-breezone` (nos conventions)
- [x] Ressources de texte en place (`composeResources/values` en français, `values-en` en anglais,
      classe `com.breezone.app.resources.Res`)
- [x] Textes des écrans déplacés vers ces ressources (175 clés, français et anglais)
- [x] Revue de performance avec le skill Google : clés et `contentType` des listes, `drawWithCache`
      et lecture pendant le dessin pour le cadran et le graphique, bordure animée lue pendant le
      dessin, anti-rebond de la consigne sans relancer d'effet, captures périmées corrigées
      (`rememberUpdatedState`), sélections en `Set`
- [ ] Tests d'interface Compose ; image système Android arm64 pour l'émulateur (aucune n'est
      installée sur la machine de dev)

### Langue

- [ ] Choisir la langue de l'application mobile (aujourd'hui : celle du téléphone, français ou
      anglais) et de l'interface web (aujourd'hui : français seulement) — réglage par utilisateur,
      enregistré sur le serveur ; textes du web déplacés dans des fichiers de traduction (fr, en)

### 6. Plus tard

- [ ] Profil intelligent météo + Tempo
- [ ] Notifications (perte de connexion, alertes Airzone, pile faible)
- [x] Programmations de l'application Airzone dans Breezone : synchronisées toutes les heures (et
      à la demande, et à la liaison), créées / modifiées / activées / supprimées sur Airzone Cloud,
      exécutées par Airzone (l'API locale du WebServer n'a pas de programmations)
- [ ] Programmations Airzone par dates (calendrier) : modification dans Breezone
- [ ] Application mobile : section « Programmations Airzone »
- [x] Onglet **Routines** (web) : minuteries (« dans 2 h, éteindre » une zone, un groupe, une
      installation ou tout), scènes dont chaque action peut attendre (« après 2 h »)
- [x] Une seule notion de programmation : celles d'Airzone (exécutées par Airzone, éditées dans
      Breezone). Les routines Breezone ne sont plus programmables : elles se lancent à la demande
- [ ] Import des routines Airzone en scènes Breezone — seulement pour un compte abonné : sans
      abonnement, Airzone Cloud répond « 402 An active subscription is required » (vérifié le
      2026-11-06) ; les routines Breezone les remplacent gratuitement
- [ ] Application mobile : minuteries et scènes avec délai
