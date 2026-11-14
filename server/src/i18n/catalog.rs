//! Translations of the fixed API error messages, looked up by their text in the code (French or
//! English). Messages read by programs stay untranslated: "invalid username or password" (the web
//! UI and the app show their own words), "share expired", "share revoked" and
//! "pairing pending approval" (read by the other node).

use super::Text;

pub fn lookup(source: &str) -> Option<Text> {
    let (fr, en, es) = match source {
        "1 to 100 actions expected" => ("1 à 100 actions attendues", "1 to 100 actions expected", "se esperan de 1 a 100 acciones"),
        "a routine needs 1 to 100 actions" => ("une routine compte de 1 à 100 actions", "a routine needs 1 to 100 actions", "una rutina necesita de 1 a 100 acciones"),
        "actualisez d'abord les programmations" => ("actualisez d'abord les programmations", "refresh the schedules first", "actualice primero las programaciones"),
        "administrator role required" => ("réservé aux administrateurs", "administrator role required", "reservado a los administradores"),
        "adresse invalide : elle doit commencer par https:// (ou http:// sur le réseau local)" => (
            "adresse invalide : elle doit commencer par https:// (ou http:// sur le réseau local)",
            "invalid address: it must start with https:// (or http:// on the local network)",
            "dirección no válida: debe empezar por https:// (o http:// en la red local)",
        ),
        "adresse invalide : https:// ou http:// attendu" => {
            ("adresse invalide : https:// ou http:// attendu", "invalid address: https:// or http:// expected", "dirección no válida: se espera https:// o http://")
        }
        "adresse invalide : indiquez une IP ou un nom d'hôte" => (
            "adresse invalide : indiquez une IP ou un nom d'hôte",
            "invalid address: enter an IP address or a host name",
            "dirección no válida: indique una IP o un nombre de host",
        ),
        "adresse invalide" => ("adresse invalide", "invalid address", "dirección no válida"),
        "Airzone Cloud a refusé ces identifiants" => {
            ("Airzone Cloud a refusé ces identifiants", "Airzone Cloud refused these credentials", "Airzone Cloud rechazó estas credenciales")
        }
        "Airzone Cloud n'a pas renvoyé de jeton" => ("Airzone Cloud n'a pas renvoyé de jeton", "Airzone Cloud did not return a token", "Airzone Cloud no devolvió ningún token"),
        "already exists" => ("existe déjà", "already exists", "ya existe"),
        "ancien format de jeton : créez un nouveau partage sur l'autre serveur (mis à jour)" => (
            "ancien format de jeton : créez un nouveau partage sur l'autre serveur (mis à jour)",
            "old token format: create a new share on the other (updated) server",
            "formato de token antiguo: cree un nuevo uso compartido en el otro servidor (actualizado)",
        ),
        "aucun compte Airzone mémorisé" => ("aucun compte Airzone mémorisé", "no Airzone account remembered", "ninguna cuenta de Airzone guardada"),
        "aucune demande d'accès en attente pour ce partage" => {
            ("aucune demande d'accès en attente pour ce partage", "no access request pending for this share", "ninguna solicitud de acceso pendiente para este uso compartido")
        }
        "aucune zone pilotable dans cette cible" => ("aucune zone pilotable dans cette cible", "no controllable zone in this target", "ninguna zona controlable en este destino"),
        "body too large" => ("requête trop volumineuse", "body too large", "solicitud demasiado grande"),
        "Breezone is already set up" => ("Breezone est déjà configuré", "Breezone is already set up", "Breezone ya está configurado"),
        "cannot delete the last administrator" => {
            ("impossible de supprimer le dernier administrateur", "cannot delete the last administrator", "no se puede eliminar al último administrador")
        }
        "cannot demote the last administrator" => {
            ("impossible de retirer son rôle au dernier administrateur", "cannot demote the last administrator", "no se puede quitar el rol al último administrador")
        }
        "ce jeton a été créé par ce serveur : utilisez-le sur l'autre serveur" => (
            "ce jeton a été créé par ce serveur : utilisez-le sur l'autre serveur",
            "this token was created by this server: use it on the other server",
            "este token lo creó este servidor: úselo en el otro servidor",
        ),
        "ce jeton ne demande pas de partage réciproque" => {
            ("ce jeton ne demande pas de partage réciproque", "this token does not ask for a reciprocal share", "este token no pide un uso compartido recíproco")
        }
        "ce partage est en lecture seule" => ("ce partage est en lecture seule", "this share is read-only", "este uso compartido es de solo lectura"),
        "cette installation est partagée en lecture seule par son nœud" => (
            "cette installation est partagée en lecture seule par son nœud",
            "this installation is shared read-only by its node",
            "su nodo comparte esta instalación en solo lectura",
        ),
        "choisissez au moins un jour" => ("choisissez au moins un jour", "choose at least one day", "elija al menos un día"),
        "choisissez au moins une installation" => ("choisissez au moins une installation", "choose at least one installation", "elija al menos una instalación"),
        "choisissez au moins une zone" => ("choisissez au moins une zone", "choose at least one zone", "elija al menos una zona"),
        "choisissez une action : allumer, éteindre ou un mode" => {
            ("choisissez une action : allumer, éteindre ou un mode", "choose an action: switch on, switch off or a mode", "elija una acción: encender, apagar o un modo")
        }
        "choisissez une action" => ("choisissez une action", "choose an action", "elija una acción"),
        "choisissez une installation : « toutes » est réservé aux administrateurs" => (
            "choisissez une installation : « toutes » est réservé aux administrateurs",
            "choose an installation: “all” is reserved to administrators",
            "elija una instalación: «todas» está reservado a los administradores",
        ),
        "connectez le compte Airzone (Réglages › Compte Airzone) en mémorisant le mot de passe" => (
            "connectez le compte Airzone (Réglages › Compte Airzone) en mémorisant le mot de passe",
            "connect the Airzone account (Settings › Airzone account) and remember the password",
            "conecte la cuenta de Airzone (Ajustes › Cuenta de Airzone) guardando la contraseña",
        ),
        "current password is incorrect" => ("mot de passe actuel incorrect", "current password is incorrect", "la contraseña actual no es correcta"),
        "database error" => ("erreur de base de données", "database error", "error de base de datos"),
        "délai entre 0 minute et 7 jours" => ("délai entre 0 minute et 7 jours", "delay between 0 minutes and 7 days", "retraso entre 0 minutos y 7 días"),
        "délai entre 1 minute et 7 jours" => ("délai entre 1 minute et 7 jours", "delay between 1 minute and 7 days", "retraso entre 1 minuto y 7 días"),
        "droit attendu : read, control ou admin" => {
            ("droit attendu : read, control ou admin", "permission expected: read, control or admin", "permiso esperado: read, control o admin")
        }
        "durée de validité invalide" => ("durée de validité invalide", "invalid validity period", "periodo de validez no válido"),
        "e-mail et mot de passe Airzone requis" => {
            ("e-mail et mot de passe Airzone requis", "Airzone e-mail and password required", "se requieren el correo y la contraseña de Airzone")
        }
        "empreinte de certificat invalide" => ("empreinte de certificat invalide", "invalid certificate fingerprint", "huella de certificado no válida"),
        "empty command" => ("commande vide", "empty command", "orden vacía"),
        "every action must change at least one setting" => {
            ("chaque action doit changer au moins un réglage", "every action must change at least one setting", "cada acción debe cambiar al menos un ajuste")
        }
        "expected a Bearer token" => ("jeton Bearer attendu", "expected a Bearer token", "se espera un token Bearer"),
        "heure invalide (HH:MM)" => ("heure invalide (HH:MM)", "invalid time (HH:MM)", "hora no válida (HH:MM)"),
        "host must be an IP address or a hostname" => {
            ("l'hôte doit être une adresse IP ou un nom d'hôte", "host must be an IP address or a hostname", "el host debe ser una dirección IP o un nombre de host")
        }
        "HTTP n'est accepté que vers une adresse locale : utilisez l'adresse https:// du serveur" => (
            "HTTP n'est accepté que vers une adresse locale : utilisez l'adresse https:// du serveur",
            "HTTP is only accepted towards a local address: use the https:// address of the server",
            "HTTP solo se acepta hacia una dirección local: use la dirección https:// del servidor",
        ),
        "indiquez l'adresse à laquelle l'autre serveur joindra celui-ci (Réglages › Serveurs › Adresse publique)" => (
            "indiquez l'adresse à laquelle l'autre serveur joindra celui-ci (Réglages › Serveurs › Adresse publique)",
            "enter the address at which the other server will reach this one (Settings › Servers › Public address)",
            "indique la dirección con la que el otro servidor llegará a este (Ajustes › Servidores › Dirección pública)",
        ),
        "indiquez le réseau à analyser, par exemple 192.168.1.0/24" => (
            "indiquez le réseau à analyser, par exemple 192.168.1.0/24",
            "enter the network to scan, for example 192.168.1.0/24",
            "indique la red que analizar, por ejemplo 192.168.1.0/24",
        ),
        "installation Airzone inconnue : synchronisez d'abord le compte Airzone" => (
            "installation Airzone inconnue : synchronisez d'abord le compte Airzone",
            "unknown Airzone installation: synchronise the Airzone account first",
            "instalación de Airzone desconocida: sincronice primero la cuenta de Airzone",
        ),
        "installation Airzone inconnue" => ("installation Airzone inconnue", "unknown Airzone installation", "instalación de Airzone desconocida"),
        "installation Airzone manquante" => ("installation Airzone manquante", "missing Airzone installation", "falta la instalación de Airzone"),
        "installation is disabled" => ("installation désactivée", "installation is disabled", "instalación desactivada"),
        "installation non partagée" => ("installation non partagée", "installation not shared", "instalación no compartida"),
        "installation not found" => ("installation introuvable", "installation not found", "instalación no encontrada"),
        "installation not loaded" => ("installation non chargée", "installation not loaded", "instalación no cargada"),
        "invalid fan speed" => ("vitesse de ventilation invalide", "invalid fan speed", "velocidad de ventilación no válida"),
        "invalid or expired token" => ("jeton invalide ou expiré", "invalid or expired token", "token no válido o caducado"),
        "invalid pairing request" => ("demande d'appairage invalide", "invalid pairing request", "solicitud de emparejamiento no válida"),
        "invalid port" => ("port invalide", "invalid port", "puerto no válido"),
        "invalid refresh token" => ("jeton de rafraîchissement invalide", "invalid refresh token", "token de renovación no válido"),
        "invalid signature" => ("signature invalide", "invalid signature", "firma no válida"),
        "invitation déjà utilisée : créez un nouveau partage" => {
            ("invitation déjà utilisée : créez un nouveau partage", "invitation already used: create a new share", "invitación ya utilizada: cree un nuevo uso compartido")
        }
        "invitation expirée (15 minutes) : créez un nouveau partage" => (
            "invitation expirée (15 minutes) : créez un nouveau partage",
            "invitation expired (15 minutes): create a new share",
            "invitación caducada (15 minutos): cree un nuevo uso compartido",
        ),
        "invitation inconnue ou déjà utilisée : créez un nouveau partage" => (
            "invitation inconnue ou déjà utilisée : créez un nouveau partage",
            "unknown or already used invitation: create a new share",
            "invitación desconocida o ya utilizada: cree un nuevo uso compartido",
        ),
        "jeton d'invitation invalide : copiez-le en entier depuis Réglages › Serveurs du serveur qui partage" => (
            "jeton d'invitation invalide : copiez-le en entier depuis Réglages › Serveurs du serveur qui partage",
            "invalid invitation token: copy it entirely from Settings › Servers on the sharing server",
            "token de invitación no válido: cópielo entero desde Ajustes › Servidores del servidor que comparte",
        ),
        "l'autre serveur n'a pas envoyé sa clé publique : mettez-le à jour" => (
            "l'autre serveur n'a pas envoyé sa clé publique : mettez-le à jour",
            "the other server did not send its public key: update it",
            "el otro servidor no envió su clave pública: actualícelo",
        ),
        "le certificat a changé entre-temps : examinez-le de nouveau" => (
            "le certificat a changé entre-temps : examinez-le de nouveau",
            "the certificate changed in the meantime: review it again",
            "el certificado cambió mientras tanto: revíselo de nuevo",
        ),
        "le libellé doit faire de 1 à 60 caractères" => {
            ("le libellé doit faire de 1 à 60 caractères", "the label must be 1 to 60 characters long", "la etiqueta debe tener de 1 a 60 caracteres")
        }
        "le nom du serveur doit faire de 1 à 60 caractères" => {
            ("le nom du serveur doit faire de 1 à 60 caractères", "the server name must be 1 to 60 characters long", "el nombre del servidor debe tener de 1 a 60 caracteres")
        }
        "les routines se lancent à la demande ; pour une heure fixe, créez une programmation (onglet Programmations)" => (
            "les routines se lancent à la demande ; pour une heure fixe, créez une programmation (onglet Programmations)",
            "routines run on demand; for a fixed time, create a schedule (Schedules tab)",
            "las rutinas se ejecutan a demanda; para una hora fija, cree una programación (pestaña Programaciones)",
        ),
        "minuterie introuvable" => ("minuterie introuvable", "timer not found", "temporizador no encontrado"),
        "missing Authorization header" => ("en-tête Authorization manquant", "missing Authorization header", "falta el encabezado Authorization"),
        "name must be 1-80 characters" => ("le nom doit faire de 1 à 80 caractères", "name must be 1-80 characters", "el nombre debe tener de 1 a 80 caracteres"),
        "node not found" | "nœud introuvable" => ("nœud introuvable", "node not found", "nodo no encontrado"),
        "node not stored" => ("nœud non enregistré", "node not stored", "nodo no guardado"),
        "partage introuvable" => ("partage introuvable", "share not found", "uso compartido no encontrado"),
        "password must be at least 8 characters" => {
            ("le mot de passe doit faire au moins 8 caractères", "password must be at least 8 characters", "la contraseña debe tener al menos 8 caracteres")
        }
        "port invalide" => ("port invalide", "invalid port", "puerto no válido"),
        "programmation introuvable : actualisez" => ("programmation introuvable : actualisez", "schedule not found: refresh", "programación no encontrada: actualice"),
        "refresh token expired" => ("jeton de rafraîchissement expiré", "refresh token expired", "token de renovación caducado"),
        "replayed request" => ("requête rejouée", "replayed request", "solicitud repetida"),
        "request too old: check the clocks of both servers" => (
            "requête trop ancienne : vérifiez l'horloge des deux serveurs",
            "request too old: check the clocks of both servers",
            "solicitud demasiado antigua: compruebe el reloj de ambos servidores",
        ),
        "role must be 'admin' or 'user'" => ("rôle attendu : admin ou user", "role must be 'admin' or 'user'", "rol esperado: admin o user"),
        "routine not found" => ("routine introuvable", "routine not found", "rutina no encontrada"),
        "session not found" => ("session introuvable", "session not found", "sesión no encontrada"),
        "session revoked" => ("session révoquée", "session revoked", "sesión revocada"),
        "signed request required" => ("requête signée requise", "signed request required", "se requiere una solicitud firmada"),
        "sleep must be 0, 30, 60 or 90 minutes" => {
            ("la veille doit être de 0, 30, 60 ou 90 minutes", "sleep must be 0, 30, 60 or 90 minutes", "el reposo debe ser de 0, 30, 60 o 90 minutos")
        }
        "timer not stored" => ("minuterie non enregistrée", "timer not stored", "temporizador no guardado"),
        "too many failed attempts, retry in a few minutes" => (
            "trop de tentatives échouées, réessayez dans quelques minutes",
            "too many failed attempts, retry in a few minutes",
            "demasiados intentos fallidos, vuelva a intentarlo en unos minutos",
        ),
        "unexpected backend" => ("accès inattendu à l'installation", "unexpected backend", "acceso inesperado a la instalación"),
        "user no longer exists" => ("cet utilisateur n'existe plus", "user no longer exists", "este usuario ya no existe"),
        "user not found" => ("utilisateur introuvable", "user not found", "usuario no encontrado"),
        "username must be 2-40 characters (letters, digits, . _ - @)" => (
            "l'identifiant doit faire de 2 à 40 caractères (lettres, chiffres, . _ - @)",
            "username must be 2-40 characters (letters, digits, . _ - @)",
            "el usuario debe tener de 2 a 40 caracteres (letras, cifras, . _ - @)",
        ),
        "version de jeton non prise en charge : mettez Breezone à jour" => {
            ("version de jeton non prise en charge : mettez Breezone à jour", "unsupported token version: update Breezone", "versión de token no compatible: actualice Breezone")
        }
        "vous avez accès à cette installation en lecture seule" => {
            ("vous avez accès à cette installation en lecture seule", "you have read-only access to this installation", "tiene acceso de solo lectura a esta instalación")
        }
        "you cannot delete your own account" => ("vous ne pouvez pas supprimer votre propre compte", "you cannot delete your own account", "no puede eliminar su propia cuenta"),
        "zone not found (installation not reachable yet?)" => {
            ("zone introuvable (installation pas encore joignable ?)", "zone not found (installation not reachable yet?)", "zona no encontrada (¿instalación aún no accesible?)")
        }
        "zone not found" => ("zone introuvable", "zone not found", "zona no encontrada"),
        "vous n'avez pas le droit de renommer ni de regrouper les zones" => (
            "vous n'avez pas le droit de renommer ni de regrouper les zones",
            "you are not allowed to rename or regroup the zones",
            "no tiene permiso para renombrar ni agrupar las zonas",
        ),
        "vous n'avez pas le droit de modifier les routines" => {
            ("vous n'avez pas le droit de modifier les routines", "you are not allowed to change the routines", "no tiene permiso para modificar las rutinas")
        }
        "vous n'avez pas le droit de modifier les programmations" => {
            ("vous n'avez pas le droit de modifier les programmations", "you are not allowed to change the schedules", "no tiene permiso para modificar las programaciones")
        }
        _ => return None,
    };
    Some(Text { fr: fr.into(), en: en.into(), es: es.into() })
}
