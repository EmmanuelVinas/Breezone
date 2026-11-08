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
- **Autres marques** (étude web du 2026-11-08, rien de testé sur un appareil réel) :
  1. Mitsubishi Electric (Kumo / Comfort) : API locale de l'adaptateur Wi-Fi, non documentée mais
     suivie par pykumo et Home Assistant ; le cloud ne sert qu'aux identifiants.
  2. Midea et ses marques : contrôle local (midea-local), un appel unique au cloud pour la clé des
     appareils V3 ; la plus large couverture pour l'effort.
  3. Daikin : API locale fragile, selon l'adaptateur et le firmware (supprimée à partir de 2.8.0).
  4. Passerelles Intesis (Fujitsu, Toshiba, Hitachi…) : fiables, sans cloud, mais matériel payant.
  5. Samsung, LG : écartés, cloud seulement.
- **Plus tard** : notifications (perte de connexion, alertes, piles), profil météo + Tempo,
  programmations Airzone par dates, HTTPS local clé en main.
