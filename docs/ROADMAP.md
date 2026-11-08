# Feuille de route

État au 2026-11-08.

## Fait

- **Serveur** (Rust, Docker multi-architecture) : API locale Airzone, temps réel, historique, journal,
  import du compte Airzone, recherche réseau.
- **Routines** à la demande (actions différées, minuteries) et **programmations Airzone** synchronisées
  et modifiables depuis Breezone.
- **Plusieurs sites** : appairage signé entre serveurs, partages réciproques, certificats acceptés après
  vérification, choix des installations affichées.
- **Utilisateurs** : accès par installation, en lecture seule ou en pilotage.
- **Interface web** et **application Android / iOS** (français, anglais, espagnol ; démo intégrée ;
  routines et programmations par installation).
- **Diffusion** : guide Synology, site breezone.cloud, kit de publication Google Play.

## À venir

- **Versions d'API** : l'application signale qu'elle ou le serveur doit être mis à jour, en restant
  rétrocompatible autant que possible.
- **Publication** : Google Play (test fermé puis production), puis App Store.
- **Langue** : choix dans le profil, web traduit (aujourd'hui en français seulement).
- **Application mobile** : scan du QR code d'appairage, minuteries et scènes avec délai, adresse locale
  et distante avec bascule automatique, tests d'interface.
- **Graphiques** comme dans l'application Airzone : confort jour par jour, mini-courbes sur les cartes,
  qualité d'air, consommation.
- **Entre serveurs** : WebSocket au lieu de la lecture périodique.
- **Plus tard** : notifications (perte de connexion, alertes, piles), profil météo + Tempo,
  programmations Airzone par dates, HTTPS local clé en main.
