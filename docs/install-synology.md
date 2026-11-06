# Installer Breezone sur un NAS Synology

Guide pour **DSM 7.2 ou plus récent** avec le paquet **Container Manager**. Breezone utilise
quelques Mo de RAM : n'importe quel modèle compatible Container Manager convient (processeur Intel,
AMD ou ARM 64 bits).

> **À savoir : n'utilisez pas l'onglet « Registre ».** Il ne sait chercher que sur Docker Hub et
> affiche « Registry returned bad result » avec GitHub Container Registry (`ghcr.io`), où Breezone
> est publié. Ce n'est pas nécessaire : le projet ci-dessous télécharge l'image tout seul.

## 1. Préparer

1. **Centre de paquets** › installez **Container Manager**.
2. **File Station** › dans le dossier partagé `docker` (créé par Container Manager), créez un dossier
   `breezone`.
3. Notez l'adresse IP du NAS (**Panneau de configuration › Réseau**), par exemple `192.168.1.20`.

## 2. Créer le projet

**Container Manager › Projet › Créer** :

- **Nom du projet** : `breezone`
- **Chemin** : `/docker/breezone`
- **Source** : *Créer un fichier docker-compose.yml*, puis collez :

```yaml
services:
  breezone:
    image: ghcr.io/emmanuelvinas/breezone:latest
    container_name: breezone
    restart: unless-stopped
    # Réseau de l'hôte : Breezone trouve tout seul les WebServers Airzone du réseau local.
    network_mode: host
    environment:
      TZ: Europe/Paris
      BREEZONE_PORT: "8080"    # changez-le si le port 8080 est déjà pris sur le NAS
    volumes:
      - breezone_data:/data
    mem_limit: 300m

volumes:
  breezone_data:
```

Cliquez sur **Suivant**, passez l'écran « Web Station », puis **Terminé** : Container Manager
télécharge l'image et démarre le conteneur (une minute environ).

## 3. Premier démarrage

Ouvrez `http://192.168.1.20:8080` (l'IP de votre NAS) :

1. créez le compte administrateur ;
2. connectez votre compte Airzone (facultatif) ou laissez Breezone rechercher les WebServers sur le
   réseau (*Réglages › Installations*).

Si la page ne s'ouvre pas : **Panneau de configuration › Sécurité › Pare-feu**, autorisez le port
`8080` (TCP) depuis votre réseau local.

## 4. Application mobile

Dans l'application, l'adresse du serveur est `http://192.168.1.20:8080` à la maison. Sur Android 17
et plus, acceptez l'autorisation « Appareils à proximité » (accès au réseau local) au premier
lancement.

## 5. Accès depuis l'extérieur (facultatif)

Deux solutions, sans ouvrir le port 3000 des WebServers (leur API n'a aucune authentification) :

- **Proxy inversé du NAS**, si vous avez déjà un nom de domaine et un certificat sur le NAS :
  **Panneau de configuration › Portail de connexion › Avancé › Proxy inversé › Créer** :
  source `HTTPS` / `breezone.mondomaine.fr` / port `443`, destination `HTTP` / `localhost` / `8080`.
  Dans **En-tête personnalisé**, cliquez sur **Créer › WebSocket** (indispensable pour le temps réel).
- **Cloudflare Tunnel**, sans ouvrir aucun port sur la box : ajoutez au projet le service
  `cloudflared` décrit dans le [docker-compose.yml](../docker-compose.yml) du dépôt, avec le jeton de
  votre tunnel pointant vers `http://localhost:8080`.

## 6. Mettre à jour

**Container Manager › Projet › breezone › Action › Arrêter**, puis **Action › Construire** : l'image
`latest` est téléchargée à nouveau et le conteneur recréé. Les données sont conservées dans le
volume `breezone_data`.

Ou en SSH : `cd /volume1/docker/breezone && sudo docker compose pull && sudo docker compose up -d`.

Pour rester sur une version précise, remplacez `latest` par son numéro (ex. `0.1.0`).

## 7. Sauvegarder

Les données (base SQLite, secret de signature) sont dans le volume `breezone_data`. En SSH :

```bash
sudo docker run --rm -v breezone_breezone_data:/data -v /volume1/docker/breezone:/backup \
  alpine tar czf /backup/breezone-backup.tgz -C /data .
```

Le fichier `breezone-backup.tgz` du dossier `docker/breezone` peut ensuite être sauvegardé avec
Hyper Backup.

## Problèmes fréquents

| Symptôme | Solution |
|----------|----------|
| « Registry returned bad result » | Normal avec `ghcr.io` : n'utilisez pas l'onglet Registre, créez le projet (étape 2). |
| Le téléchargement de l'image échoue (`denied`, `unauthorized`) | L'image n'est pas encore publique, ou le nom est mal saisi : `ghcr.io/emmanuelvinas/breezone` (en minuscules). |
| Le conteneur redémarre en boucle, « address already in use » | Le port 8080 est pris : changez `BREEZONE_PORT` (ex. `8099`). |
| Aucun WebServer trouvé | Vérifiez `network_mode: host` ; sinon indiquez le réseau dans *Réglages › Installations* (ex. `192.168.1.0/24`) ou l'adresse IP du WebServer. |
| L'application Android affiche une erreur de réseau local | Autorisez « Appareils à proximité » dans *Paramètres › Applications › Breezone › Autorisations*. |
