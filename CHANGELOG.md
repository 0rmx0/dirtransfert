# Changelog

Les versions notables de DirTransfert. Format inspiré de [Keep a Changelog](https://keepachangelog.com/fr/).

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
