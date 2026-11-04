# DirTransfert

[![License: MIT](https://img.shields.io/badge/License-MIT-magenta.svg)](LICENSE)

Outil de transfert d'un répertoire complet (et ses sous-répertoires) entre deux PC, **décentralisé** : aucun serveur tiers, le transfert se fait en direct (TCP) entre les deux machines. Conçu pour plusieurs milliers de fichiers et plusieurs dizaines de Go.

- **GUI** (Slint) par défaut, **CLI** derrière `--cli`
- Windows, **Linux**, macOS
- Zéro dépendance externe pour la logique métier : protocole et UPnP écrits à la main (le chiffrement délègue aux crates RustCrypto `chacha20poly1305`, `argon2`, `rand`, et le hachage au crate `sha2`)

## Fonctionnalités

- **Protocole TCP maison** (`DTX1`) : manifeste binaire, puis transfert fichier par fichier à la demande du récepteur.
- **Intégrité SHA-256** : chaque fichier est haché à la source, vérifié à l'écriture et re-vérifié en fin de transfert. Écriture atomique (fichier temporaire `.dtxpart` puis renommage) : jamais de fichier à moitié écrit.
- **Chiffrement du flux** : tout le transfert est chiffré en **ChaCha20-Poly1305** (confidentialité + intégrité de chaque trame), clé dérivée du **code de jumelage** via **Argon2id**. Le code sert aussi d'authentification : une connexion sans le bon code est refusée avant tout transfert.
- **Reprise sur interruption, même au niveau d'un gros fichier** : chaque morceau de 1 Mo est haché individuellement ; si le transfert est coupé, relancez les deux côtés — les fichiers déjà valides sont ignorés et un fichier partiellement reçu reprend là où il en était (`.dtxpart` vérifié morceau par morceau).
- **Approbation du manifeste** : le récepteur voit la liste des fichiers et la taille totale avant d'accepter (boutons Accepter/Refuser en GUI, confirmation interactive en CLI, `--yes` pour automatiser).
- **Découverte LAN** : en mode envoi, `--discover` trouve les récepteurs DirTransfert du réseau local par sonde UDP signée (le code n'est jamais exposé sur le réseau). Côté réception, un QR code `dtx://IP:port?code=...` et la commande prête à copier sont affichés.
- **Barre de progression** : octets transférés, débit Mo/s et pourcentage en temps réel (GUI avec vraie barre de progression, journal plafonné).
- **Filtres et options d'envoi** : `--exclude` (motifs `*`/`?`), `--dry-run` (liste sans envoyer), `--limit` (débit max en Mo/s), `--compress` (zstd par morceau, feature `compress`), envoi vers **plusieurs destinataires** en une commande, `--json` pour les scripts, `verify` pour re-vérifier une réception, historique local et fichier de config `~/.config/dirtransfert/config`.
- **Sécurité des chemins** : les chemins relatifs sont filtrés (pas de `..`), pas de traversée de répertoire possible.
- **Réception assistée** : affichage de l'IP locale et de l'IP publique (via UPnP), ouverture automatique du pare-feu Windows et redirection de port sur la box.

## Binaires pré-compilés (Releases)

Chaque version est publiée avec des binaires prêts à l'emploi sur la [page des releases](https://github.com/0rmx0/dirtransfert/releases) :

- `DirTransfert-linux-amd64.tar.gz` / `DirTransfert-linux-arm64.tar.gz` (Linux x64 / ARM64)
- `DirTransfert-windows-amd64.zip` (Windows x64)
- `DirTransfert-macos-amd64.tar.gz` / `DirTransfert-macos-arm64.tar.gz` (macOS Intel / Apple Silicon)
- `dirtransfer_<version>_amd64.deb` (paquet Debian/Ubuntu, avec raccourci applicatif)

Chaque archive est accompagnée de sa somme SHA-256 (fichier `.sha256`).

> **Important — incompatibilité 0.2.0** : le protocole de trame a changé en `0.2.0` (trames typées, transfert par morceaux). Une version `>= 0.2.0` **ne peut pas** échanger avec une version `< 0.2.0` : mettez à jour les deux côtés.

> Depuis `0.7.0`, le protocole passe à `DTX2` (manifeste avec hash par morceau de 1 Mo, requêtes avec offset de reprise, sel/challenge de session, code à 10 caractères) : une version `>= 0.7.0` **ne peut pas** échanger avec une version `< 0.7.0`.

## Compilation

Installez [Rust](https://rustup.rs) si besoin :

```sh
git clone https://github.com/0rmx0/dirtransfert
cd dirtransfer
cargo build --release
```

Le binaire est `target/release/DirTransfert` (`.exe` sous Windows). C'est un fichier autonome, aucun runtime requis.

> Sous Linux, la GUI (Slint) requiert les bibliothèques graphiques standard (fontconfig, Wayland/X11) — installées par défaut sur la plupart des distributions de bureau (Ubuntu inclus).

## Utilisation

### Interface graphique (par défaut)

Lancez le binaire sans argument :

```sh
./DirTransfert        # ou DirTransfert.exe sous Windows
```

Une fenêtre s'ouvre avec :
- le choix du mode **Envoyer / Recevoir** ;
- la sélection du répertoire via un sélecteur de dossiers natif ;
- l'adresse et le port de destination ;
- les options UPnP / pare-feu en cases à cocher (côté réception) ;
- le **code de jumelage** : saisi des deux côtés (pré-généré au lancement côté réception, régénérable via le bouton « Générer »), il chiffre et authentifie le transfert ;
- un bouton **Annuler** pour interrompre un transfert en cours ;
- une barre de progression (octets, Mo/s, %) et un journal en direct.

Côté réception, l'outil affiche l'adresse exacte à communiquer à l'émetteur (IP locale et IP publique via UPnP), et alerte en cas de CGNAT.

Sous Windows, pour l'ouverture du pare-feu, lancez l'exe **en tant qu'administrateur** (clic droit → « Exécuter en tant qu'administrateur ») ; sinon l'outil affiche un avertissement dans le journal et continue.

### Ligne de commande (`--cli`)

**1. Sur le PC récepteur** (sous Windows, console **ouverte en tant qu'administrateur**) :

```sh
DirTransfert --cli receive 0.0.0.0:47500 /reception --upnp --open-fw --close-fw
```

Sans `--code`, le récepteur génère un code de jumelage et l'affiche :

```
code de jumelage : AB2CD-34567  (à saisir sur l'émetteur)
```

L'outil ouvre alors automatiquement :
- la **règle entrante dans le pare-feu Windows** (`--open-fw`, règle nommée `DirTransfert`, port TCP 47500) ;
- la **redirection de port sur la box** (`--upnp`, via UPnP-IGD : découverte SSDP de la box, `AddPortMapping`, récupération de l'IP publique).

Il affiche ensuite la **commande `send` prête à copier** avec l'IP publique de la box.

**2. Sur le PC émetteur** :

```sh
DirTransfert --cli send /donnees 82.65.12.34:47500 --code AB2CD-34567
```

Le répertoire est recréé sous `/reception/donnees` avec toute son arborescence. À la fin du transfert, avec `--close-fw`, la règle pare-feu et la redirection UPnP sont **automatiquement fermées**.

**En cas d'interruption** : relancez simplement `receive` puis `send` — la reprise est automatique, **y compris au milieu d'un gros fichier** (morceaux de 1 Mo vérifiés un par un).

**Sur le même réseau local**, sans taper d'adresse ni de code à la main côté GUI : lancez `receive` sur un PC, puis sur l'autre cliquez **Découvrir** (ou `send <dir> --discover --code XXXXX-XXXXX` en CLI) — la sonde UDP signée ne révèle pas le code. Côté réception, un **QR code** `dtx://IP:port?code=...` et la commande prête à copier sont affichés.

**Vérifier une réception après coup** :

```sh
DirTransfert --cli verify /donnees /reception
```

### Options communes

| Option | Effet |
|---|---|
| `--code XXXXX-XXXXX` | Code de jumelage 10 caractères : chiffre le flux et authentifie la connexion (obligatoire côté émetteur, généré si omis côté récepteur) |
| `--json` | Sortie machine : lignes JSON sur stdout (`log`, `progress`, `done`, `error`) |
| `--exclude <motif>` | Exclut les fichiers correspondant au motif (`*` = toute séquence, `?` = 1 caractère ; répétable ; un motif sans `/` matche aussi les dossiers, ex. `node_modules`) |
| `--limit <Mo/s>` | Limite le débit d'envoi |
| `--compress` | Compresse chaque morceau (zstd) avant chiffrement — nécessite `cargo build --features compress` |

### Options du récepteur

| Option | Effet |
|---|---|
| `--yes` | Accepte automatiquement le manifeste reçu (sinon confirmation interactive) |
| `--upnp` | Redirection de port automatique sur la box (UPnP-IGD, repli NAT-PMP) |
| `--open-fw` | Règle entrante automatique dans le pare-feu Windows (exige une console administrateur) |
| `--close-fw` | Ferme pare-feu + redirection UPnP après le transfert |
| `--pipeline N` | Requêtes en vol simultanées (1-64, défaut 8) : augmentez sur LAN rapide |
| `--ipv6` | Écoute sur l'adresse IPv6 globale si disponible |

### Options de l'émetteur

| Option | Effet |
|---|---|
| `--discover` | Découvre les récepteurs du réseau local (sonde UDP signée, port 47501) au lieu de saisir l'adresse |
| `--dry-run` | Liste les fichiers et la taille totale sans rien envoyer |
| `send <dir> <addr1> <addr2> ...` | Envoi vers **plusieurs récepteurs** en parallèle |

**Configuration** : `~/.config/dirtransfert/config` (lignes `clé = valeur`, `#` pour commenter) — clés `pipeline`, `exclude` (séparées par des virgules), `limit`, `auto-yes`. Historique des transferts dans `~/.local/share/dirtransfert/history.log`.

Si une étape échoue (box sans UPnP, console non administrateur), l'outil l'affiche clairement et continue — il indique alors quoi ouvrir manuellement.

## Transfert via Internet

1. **UPnP automatique** : la méthode par défaut ci-dessus fonctionne si la box a l'UPnP activé (cas de la quasi-totalité des box françaises : Livebox, Freebox, SFR Box, Bbox).
2. **VPN entre les deux PC** si l'UPnP est désactivé ou bloqué par le FAI (certains bloquent les ports entrants, notamment en CGNAT/4G) : WireGuard ou Tailscale, puis utilisez l'adresse IP du VPN comme destination.
3. Le trafic est un flux TCP brut : sur une ligne ADSL à 1 Mo/s d'upload, comptez ~4,5 h pour 16 Go ; sur du fibre symétrique, ~20-30 min.

**Note CGNAT** : si l'IP publique affichée commence par `100.64.` à `100.127.`, vous êtes derrière un CGNAT opérateur — l'UPnP ne suffira pas, utilisez le VPN.

## Chiffrement et confidentialité

Le flux est chiffré de bout en bout entre les deux PC :

- **Dérivation de clé** : Argon2id (64 Mio, 2 passes) sur le code de jumelage, avec **sel aléatoire par session** (échangé pendant la poignée de main).
- **Clé de session fraîche à chaque connexion** : HKDF-SHA256(master, hello_client ‖ challenge_serveur) — deux sessions n'utilisent jamais la même clé, et **un rejeu d'une poignée de main capturée est rejeté** (challenge serveur tiré au hasard à chaque connexion).
- **Chiffrement** : ChaCha20-Poly1305 (AEAD) — chaque trame est chiffrée, son intégrité est vérifiée (tag Poly1305) et son numéro (nonce+AAD) empêche toute réorganisation ou réinjection.
- **Authentification mutuelle** : preuve HMAC-SHA256 dans les deux sens sur (hello_client, challenge_serveur) avant le moindre octet de données ; timeout de 15 s et limite de 5 tentatives/min/IP côté récepteur contre le balayage en ligne — un intrus qui scannerait le port ne pourra pas se connecter sans le code.
- **Code de jumelage** : 10 caractères sans caractères ambigus (pas de O/0/1/I), ~50 bits d'entropie. Communiquez-le par un canal de confiance (oral, téléphone), pas par le même canal que le transfert.

Le code n'est jamais transmis sur le réseau : seuls circulent des nonces aléatoires et des preuves HMAC dérivées.

## Limites connues

- Transfert séquentiel, un fichier à la fois côté récepteur (le pipeline `--pipeline N` met plusieurs requêtes en vol).
- Clés de session fraîches à chaque connexion, mais un code réutilisé reste la racine de confiance : un attaquant qui capture une poignée de main peut tenter une recherche hors ligne (coût Argon2id). Changez de code régulièrement.
- Un seul transfert à la fois par instance réceptrice (plusieurs destinataires possibles côté envoi).

## Structure du code

- `src/main.rs` — point d'entrée : GUI par défaut, CLI derrière `--cli`/`--console`.
- `src/protocol.rs` — manifeste, encodage/décodage des messages, scan du répertoire source.
- `src/crypto.rs` — chiffrement du flux : dérivation Argon2id, AEAD ChaCha20-Poly1305, poignée de main authentifiée, `SecureStream` (Read/Write chiffrés).
- `src/upnp.rs` — client UPnP-IGD en pur Rust : découverte SSDP (hôtes privés uniquement), description XML, appels SOAP `AddPortMapping`/`GetExternalIPAddress`/`DeletePortMapping`.
- `src/firewall.rs` — règles `netsh advfirewall` pour le pare-feu Windows (règle entrante `DirTransfert`).
- `src/transfer.rs` — cœur du transfert (send/receive) : reprise par morceau, approbation du manifeste, multi-destinataires, callbacks de progression, partagé par le CLI et la GUI.
- `src/protocol.rs` — protocole DTX2 : manifeste avec hash par morceau, requêtes avec offset de reprise, `rel_to_os` durci, glob d'exclusion, scan sans suivre les liens symboliques.
- `src/crypto.rs` — handshake anti-rejeu (sel + challenge, HKDF, HMAC), `SecureStream` (Read/Write chiffrés), tag de découverte LAN.
- `src/discover.rs` — découverte LAN par sonde UDP signée (port 47501).
- `src/config.rs` — fichier de config `~/.config/dirtransfert/config` et historique local.
- `src/ui.rs` + `src/slint_main.rs` — interface graphique Slint (charte R_M_X) : UI déclarative séparée de la logique, barre de progression, QR code, approbation du manifeste, boutons Copier/Découvrir.
- `src/cli.rs` — interface en ligne de commande (`--json`, `--discover`, `--dry-run`, `--exclude`, `--limit`, `--verify`, ...).

## Tests

```sh
cargo test
```

Couvre la normalisation des codes, la poignée de main (mauvais code rejeté, **rejeu rejeté**), l'encodage/décodage du protocole (trames tronquées sans panic), `rel_to_os` (traversées, lecteurs Windows), les glob d'exclusion, la découverte LAN (sonde signée), les transferts de bout en bout (complet, reprise par morceau, manifeste refusé, IPv6 si disponible), `verify`, et le dry-run. Lancez aussi `cargo test --features compress` pour la compression.

## Licence

Distribué sous licence [MIT](LICENSE).

## Icône de l'application

L'icône R_M_X (`assets/`) est embarquée dans l'exécutable — visible dans la barre des tâches et le dock :

- **Linux** : icône de fenêtre via le protocole Wayland/X11 (aucune étape supplémentaire).
- **Windows** : en plus de l'icône de fenêtre, l'icône est embarquée dans l'exécutable via `build.rs` + `assets/icon.ico` (`winresource`). `cargo build --release` suffit ; pour la barre des tâches, Windows peut mettre en cache l'icône des exécutables renommés — lancer depuis un chemin stable.
- **macOS** : le dock n'affiche que l'icône d'un bundle `.app`. Empaqueter via `cargo bundle` ou :

```sh
mkdir -p dirtransfer.app/Contents/MacOS
cp target/release/DirTransfert dirtransfer.app/Contents/MacOS/
cp assets/icon-256.png dirtransfer.app/Contents/Resources/AppIcon.png
printf '<?xml version="1.0"?>\n<plist version="1.0"><dict><key>CFBundleExecutable</key><string>DirTransfert</string><key>CFBundleIconFile</key><string>AppIcon</string></dict></plist>' > dirtransfer.app/Contents/Info.plist
```
