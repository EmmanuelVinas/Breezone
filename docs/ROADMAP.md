# Feuille de route

État au 2026-11-11.

## Fait

- **Serveur** (Rust, Docker multi-architecture) : API locale Airzone, temps réel, historique, journal,
import du compte Airzone, recherche réseau.
- **Routines** à la demande (actions différées, minuteries) et **programmations Airzone** synchronisées
et modifiables depuis Breezone.
- **Plusieurs sites** : appairage signé entre serveurs, partages réciproques, certificats acceptés après
vérification, choix des installations affichées, suivi en direct qui se rétablit seul après une panne.
- **Utilisateurs** : accès par installation, en lecture seule ou en pilotage ; réglages du serveur
réservés aux administrateurs, qui choisissent ce que chaque utilisateur peut modifier (zones,
routines, programmations).
- **Versions d'API** : l'application signale qu'elle ou le serveur doit être mis à jour.
- **Interface web** et **application Android / iOS** (français, anglais, espagnol ; démo intégrée ;
routines et programmations par installation) ; langue du web choisie dans le profil, messages du
serveur dans la langue de chacun.
- **Graphiques** : mini-courbe de 24 h sur chaque carte de zone, confort jour par jour (températures,
part du temps à ±0,5° de la consigne), qualité de l'air des thermostats qui la mesurent, temps de
demande des zones enregistré au choix par installation.
- **Diffusion** : guide Synology, site breezone.cloud, kit de publication Google Play.

## À venir

- **Application mobile** : scan du QR code d'appairage, minuteries et scènes avec délai, adresse locale
et distante avec bascule automatique, tests d'interface.
- **Réorganiser ses zones** : changer l'ordre des zones et des groupes, déplacer une zone d'un groupe
à l'autre, sur le web et dans l'application.
- **Consommation** : modules énergie (#39).
- **Autres marques**:
  1. Mitsubishi Electric (Kumo / Comfort) : API locale de l'adaptateur Wi-Fi, non documentée mais
   suivie par pykumo et Home Assistant ; le cloud ne sert qu'aux identifiants.
  2. Midea et ses marques : contrôle local (midea-local), un appel unique au cloud pour la clé des
   appareils V3 ; la plus large couverture pour l'effort.
  3. Daikin : API locale fragile, selon l'adaptateur et le firmware (supprimée à partir de 2.8.0).
  4. Passerelles Intesis (Fujitsu, Toshiba, Hitachi…) : fiables, sans cloud, mais matériel payant.
  5. Samsung, LG : écartés, cloud seulement.
- notifications (perte de connexion, alertes, piles), 
- profil météo 
- Profile abonnement électricité (Tempo...).