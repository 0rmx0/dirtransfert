# Changelog

Les versions notables de DirTransfert. Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/).

## [Non publié]

### Modifié
- **GUI : charte Terminal R_M_X v2** : fond noir-vert `#0A0F0A`, unique teinte vert CRT `#33FF66` (+ dérivés `crt-faible`/`crt-voile` pour placeholder, désactivé, footer) : plus de néons cyan/magenta, plus de surfaces grises, plus de coins arrondis ; police HelpMe enregistrée embarquée (`include_bytes!`, famille `HelpMe`) et plein écran ; curseur bloc `█` clignotant dans l'en-tête — seul mouvement autorisé ; boutons/checkbox/champs en filaire vert (bouton primaire plein vert à texte noir-vert, bordures 1px) ; images de fond `grain.png`/`mask.png` retirées de l'interface (conservées dans `src/ui/`) ; couche cachée base64 renouvelée, toujours affichée telle quelle en pied de fenêtre.
- Logique fonctionnelle inchangée : la charte ne touche qu'au rendu ; modes, transfert, protocole DTX2, progression, QR, approbation du manifeste et journal identiques.

## [0.7.0] - 2026-11-04

> **Incompatibilité protocole** : `DTX2` (manifeste avec hash par morceau, requêtes avec reprise, handshake à sel/challenge) et code de jumelage à 10 caractères. Mettez à jour **les deux côtés**.

### Sécurité
- **Handshake anti-rejeu** : sel et challenge aléatoires par session, clé de session HKDF(master, hello_client‖challenge_serveur), preuve HMAC mutuelle — un rejeu de poignée de main capturée est rejeté (test de régression). Le code n'a plus de vérifieur statique exposé.
- Timeout de 15 s pendant la handshake ; limite de 5 tentatives de handshake/min/IP côté récepteur (le récepteur ré-attend une connexion après un mauvais code au lieu d'échouer).
- `FileEntry::decode` : bornes corrigées (plus de panic sur trame tronquée) ; `rel_to_os` renforcé (rejet des lecteurs `C:`, chemins UNC, noms de périphériques Windows réservés) ; les liens symboliques ne sont plus suivis à l'émission.
- Lectures HTTP/SSDP bornées ; SSDP n'accepte que les réponses d'hôtes privés.
- Code de jumelage passé à 10 caractères (~50 bits).

### Ajouté
- **Reprise au niveau bloc** : hash par morceau de 1 Mo dans le manifeste ; un `.dtxpart` valide en préfixe est repris là où il en était.
- **Approbation du manifeste** côté récepteur (GUI : Accepter/Refuser ; CLI : confirmation interactive, `--yes` pour automatiser ; `REFUSED` côté protocole).
- **Découverte LAN** par sonde UDP signée (HMAC/Argon2, port 47501) : `--discover` / bouton « Découvrir ».
- **QR code** `dtx://IP:port?code=...` (GUI + ASCII en CLI) et bouton « Copier la commande ».
- CLI : `--json`, `--exclude`, `--dry-run`, `--limit`, `--compress` (feature `compress`, zstd), multi-destinataires, sous-commande `verify`.
- Fichier de config `~/.config/dirtransfert/config` (pipeline, exclude, limit, auto-yes) et historique `~/.local/share/dirtransfert/history.log`.
- GUI : vraie barre de progression (événements throttlés à 100 ms), journal plafonné à 400 lignes (plus de croissance O(n²)), champs Exclure/Limite, UI extraite dans `src/ui.rs`, `invoke_from_event_loop` au lieu du timer+canal.
- CI : `cargo audit`, clippy/tests avec `--all-features`, builds release en `--locked`, signature minisign (rsign2) optionnelle des artifacts si `MINISIGN_SECRET_KEY` est configuré.

### Corrigé
- Plus de panic sur `send /` (nom de racine) ; le fichier temporaire garde le nom complet (`f.bin.dtxpart`, plus de collision `a.txt`/`a.bin`).
- Flag `--log`/`--verbose` mort supprimé ; test IPv6 auto-sauté si pas de loopback v6.


## [0.5.0] - 2026-10-23

### Ajouté
- **UI Slint avec charte graphique R_M_X** : fond noir profond `#111111`, surfaces `#222222`, accents néon cyan/magenta, vert terminal pour les données, rayon 10px, couche cachée.
- **Adresse d'écoute par défaut** (`0.0.0.0:47500`) pré-remplie en réception, effacée en mode envoi.
- **Journal avec scrollbar** : le journal s'agrandit avec la fenêtre et défile indépendamment du reste de l'interface.

### Modifié
- **Boutons fonctionnels** : mode Envoyer/Recevoir, Démarrer lit les valeurs saisies (bindings bidirectionnels), Annuler positionne le jeton d'annulation.
- **Textes non tronqués** : libellés avec elide, valeurs centrées (code, adresse, pipeline), données techniques en monospace.

### Supprimé
- **Ancienne UI egui/eframe** (`src/gui.rs`, `src/icon.rs`, binaire séparé, backend `--render=`) : l'UI Slint devient l'interface par défaut.
- **Dépendance GTK** : le sélecteur de fichiers passe au portail XDG (`xdg-portal`).

## [0.3.2] - 2026-10-23

### Ajouté
- **Détection IP publique multi-sources** : UPnP → services HTTP (ipify, ifconfig.me, AWS, icanhazip) → STUN (Google, Cloudflare).
- **Détection du mode réseau** : affiche si l'on est en direct, NAT, ou CGNAT (avec avertissement spécifique).
- **NAT-PMP/PCP** : tentative automatique de redirection de port si UPnP échoue, avec fermeture propre après le transfert.
- **Support IPv6 littéral** : adresses de la forme `[2001:db8::1]:1234` sont maintenant acceptées en entrée et générées en sortie.
- **Option `--ipv6`** (CLI) et case à cocher (GUI) pour forcer l'écoute sur l'adresse IPv6 globale si disponible.

### Modifié
- **Bouton Annuler effectif pendant l'attente de connexion** :
  - Récepteur : `accept()` non-bloquant avec polling du flag d'annulation toutes les 100 ms.
  - Émetteur : `connect_timeout` (1s) par adresse en boucle avec vérification d'annulation entre chaque tentative.
  - Résolution multi-adresses via `to_socket_addrs()` permet de retenter même si le récepteur n'est pas encore prêt.

## [0.3.1] - 2026-10-16

### Ajouté
- Option `--render=dx12|dx11|vulkan|gl|glow` pour choisir le backend graphique de la GUI (utile en RDP/RDS).
- Option `--log` / `--verbose` : active l'écriture du fichier `DirTransfert-erreur.log` (désactivé par défaut).
- Dialogue d'erreur natif en cas d'échec du lancement de la GUI (plus de fermeture silencieuse).

### Modifié
- Renderer GUI par défaut : wgpu (Direct3D/Vulkan/Metal selon la plateforme) avec repli automatique vers glow (OpenGL) en cas d'échec. Aucun backend n'est forcé.
## [0.3.0] - 2026-10-15

### Ajouté
- Bouton **Annuler** dans la GUI et jeton d'annulation dans le moteur (envoi et réception, vérifié entre chaque morceau).
- Pipeline configurable : option `--pipeline N` (CLI) et slider « Requêtes en vol » (GUI), 1-64, défaut 8.
- Code de jumelage pré-généré au lancement en mode réception.
- CHANGELOG.md ; section Releases dans le README avec note d'incompatibilité inter-versions.

### Modifié
- Pare-feu Linux : message d'erreur actionnable (`ufw` / `firewall-cmd`) avec le port réel.
- GUI : le texte chiffré du bas est réservé via `Panel::bottom` (plus de superposition avec le journal).

## [0.2.0] - 2026-10-15

### Sécurité
- Correction d'une collision de nonce entre les deux directions du flux chiffré (octet de direction intégré au compteur de trame).
- Comparaison du jeton de jumelage en temps constant.
- Timeouts de lecture/écriture (30 s) sur le flux chiffré : un pair bloqué ne gèle plus le transfert.

### Ajouté
- Transfert par morceaux de 1 Mo (`CHUNK`) : un fichier volumineux n'est plus chargé en entier en RAM.
- Fenêtre de 8 requêtes en vol côté récepteur (fini le stop-and-wait fichier par fichier).
- Statut final explicite `OK`/`FAIL` entre récepteur et émetteur.
- CI GitHub Actions (fmt, clippy, tests) et workflow de release multi-plateforme (Linux amd64/arm64, Windows amd64, macOS amd64/arm64, paquet .deb).
- Version affichée dynamiquement dans la fenêtre « À propos » (lue depuis Cargo.toml), avec défilement.

### Modifié
- **Incompatible** : format de trame 9 octets avec type (`META`/`CHUNK`) — les versions < 0.2.0 ne peuvent pas échanger avec les versions >= 0.2.0.
- Suppression de l'implémentation SHA-256 maison au profit du crate `sha2`.

## [0.1.0] - 2026-10-13

- Version initiale : GUI egui, CLI `--cli`, protocole `DTX1`, chiffrement ChaCha20-Poly1305 + Argon2id, reprise sur interruption, UPnP-IGD, pare-feu Windows, paquet Debian.
