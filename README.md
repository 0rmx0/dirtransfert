# DirTransfert

[![License: MIT](https://img.shields.io/badge/License-MIT-magenta.svg)](LICENSE)

Outil de transfert d'un répertoire complet (et ses sous-répertoires) entre deux PC, **décentralisé** : aucun serveur tiers, le transfert se fait en direct (TCP) entre les deux machines. Conçu pour plusieurs milliers de fichiers et plusieurs dizaines de Go.

- **GUI** (egui) par défaut, **CLI** derrière `--cli`
- Windows, **Linux**, macOS
- Zéro dépendance externe pour la logique métier : protocole, SHA-256 et UPnP écrits à la main (le chiffrement délègue aux crates RustCrypto `chacha20poly1305`, `argon2` et `rand`)

## Fonctionnalités

- **Protocole TCP maison** (`DTX1`) : manifeste binaire, puis transfert fichier par fichier à la demande du récepteur.
- **Intégrité SHA-256** : chaque fichier est haché à la source, vérifié à l'écriture et re-vérifié en fin de transfert. Écriture atomique (fichier temporaire `.dtxpart` puis renommage) : jamais de fichier à moitié écrit.
- **Chiffrement du flux** : tout le transfert est chiffré en **ChaCha20-Poly1305** (confidentialité + intégrité de chaque trame), clé dérivée du **code de jumelage** via **Argon2id**. Le code sert aussi d'authentification : une connexion sans le bon code est refusée avant tout transfert.
- **Reprise sur interruption** : si le transfert est coupé (coupure réseau, PC éteint), relancez les deux côtés — les fichiers déjà arrivés avec un checksum valide sont ignorés, seuls les manquants sont retransférés.
- **Barre de progression** : octets transférés, débit Mo/s et pourcentage en temps réel.
- **Sécurité des chemins** : les chemins relatifs sont filtrés (pas de `..`), pas de traversée de répertoire possible.
- **Réception assistée** : affichage de l'IP locale et de l'IP publique (via UPnP), ouverture automatique du pare-feu Windows et redirection de port sur la box.

## Compilation

Installez [Rust](https://rustup.rs) si besoin :

```sh
git clone https://github.com/0rmx0/dirtransfert
cd dirtransfer
cargo build --release
```

Le binaire est `target/release/DirTransfert` (`.exe` sous Windows). C'est un fichier autonome, aucun runtime requis.

> Sous Linux, la GUI (egui + sélecteur de fichiers GTK) requiert `libgtk-3` et OpenGL — installés par défaut sur la plupart des distributions de bureau (Ubuntu inclus).

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
- le **code de jumelage** : saisi des deux côtés (généré automatiquement côté réception via le bouton « Générer »), il chiffre et authentifie le transfert ;
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
code de jumelage : AB2C-D34F  (à saisir sur l'émetteur)
```

L'outil ouvre alors automatiquement :
- la **règle entrante dans le pare-feu Windows** (`--open-fw`, règle nommée `DirTransfert`, port TCP 47500) ;
- la **redirection de port sur la box** (`--upnp`, via UPnP-IGD : découverte SSDP de la box, `AddPortMapping`, récupération de l'IP publique).

Il affiche ensuite la **commande `send` prête à copier** avec l'IP publique de la box.

**2. Sur le PC émetteur** :

```sh
DirTransfert --cli send /donnees 82.65.12.34:47500 --code AB2C-D34F
```

Le répertoire est recréé sous `/reception/donnees` avec toute son arborescence. À la fin du transfert, avec `--close-fw`, la règle pare-feu et la redirection UPnP sont **automatiquement fermées**.

**En cas d'interruption** : relancez simplement `receive` puis `send` — la reprise est automatique.

### Options du récepteur

| Option | Effet |
|---|---|
| `--code XXXX-XXXX` | Code de jumelage : chiffre le flux et authentifie la connexion (obligatoire côté émetteur, généré si omis côté récepteur) |
| `--upnp` | Redirection de port automatique sur la box (UPnP-IGD) |
| `--open-fw` | Règle entrante automatique dans le pare-feu Windows (exige une console administrateur) |
| `--close-fw` | Ferme pare-feu + redirection UPnP après le transfert |

Si une étape échoue (box sans UPnP, console non administrateur), l'outil l'affiche clairement et continue — il indique alors quoi ouvrir manuellement.

## Transfert via Internet

1. **UPnP automatique** : la méthode par défaut ci-dessus fonctionne si la box a l'UPnP activé (cas de la quasi-totalité des box françaises : Livebox, Freebox, SFR Box, Bbox).
2. **VPN entre les deux PC** si l'UPnP est désactivé ou bloqué par le FAI (certains bloquent les ports entrants, notamment en CGNAT/4G) : WireGuard ou Tailscale, puis utilisez l'adresse IP du VPN comme destination.
3. Le trafic est un flux TCP brut : sur une ligne ADSL à 1 Mo/s d'upload, comptez ~4,5 h pour 16 Go ; sur du fibre symétrique, ~20-30 min.

**Note CGNAT** : si l'IP publique affichée commence par `100.64.` à `100.127.`, vous êtes derrière un CGNAT opérateur — l'UPnP ne suffira pas, utilisez le VPN.

## Chiffrement et confidentialité

Le flux est chiffré de bout en bout entre les deux PC :

- **Dérivation de clé** : Argon2id (64 Mio, 2 passes) sur le code de jumelage, avec sel déterministe par code.
- **Chiffrement** : ChaCha20-Poly1305 (AEAD) — chaque trame est chiffrée, son intégrité est vérifiée (tag Poly1305) et son numéro (nonce+AAD) empêche toute réorganisation ou réinjection.
- **Authentification** : le code de jumelage doit être identique des deux côtés. La poignée de main vérifie la connaissance du code **dans les deux sens** avant le moindre octet de donnée — un intrus qui scannerait le port ne pourra pas se connecter sans le code.
- **Code de jumelage** : 8 caractères sans caractères ambigus (pas de O/0/1/I), ~40 bits d'entropie. Communiquez-le par un canal de confiance (oral, téléphone), pas par le même canal que le transfert.

Le code n'est jamais transmis sur le réseau : seule une preuve SHA-256 (hello aléatoire + jeton dérivé) circule.

## Limites connues

- Transfert séquentiel, un fichier à la fois (fiable et simple, mais n'utilise pas le lien en parallèle si les fichiers sont très petits).
- Pas de transfert-forward secrecy : un code réutilisé maintient la même clé. Pour un usage répété, changez de code à chaque transfert.
- Un seul transfert à la fois par instance (le récepteur accepte une unique connexion).

## Structure du code

- `src/main.rs` — point d'entrée : GUI par défaut, CLI derrière `--cli`/`--console`.
- `src/protocol.rs` — manifeste, encodage/décodage des messages, scan du répertoire source.
- `src/crypto.rs` — chiffrement du flux : dérivation Argon2id, AEAD ChaCha20-Poly1305, poignée de main authentifiée, `SecureStream` (Read/Write chiffrés).
- `src/sha256.rs` — implémentation SHA-256 en pur Rust (testée contre les vecteurs NIST).
- `src/upnp.rs` — client UPnP-IGD en pur Rust : découverte SSDP, description XML, appels SOAP `AddPortMapping`/`GetExternalIPAddress`/`DeletePortMapping`.
- `src/firewall.rs` — règles `netsh advfirewall` pour le pare-feu Windows (règle entrante `DirTransfert`).
- `src/transfer.rs` — cœur du transfert (send/receive) avec callbacks de progression, partagé par le CLI et la GUI.
- `src/gui.rs` — interface graphique egui/eframe (renderer OpenGL), transfert et dialogue de sélection de dossier dans des threads dédiés.
- `src/cli.rs` — interface en ligne de commande.

## Tests

```sh
cargo test
```

Vérifie l'implémentation SHA-256 (vecteurs « abc », vide, et 1 Mo de « a »), le protocole, les endpoints, l'état initial de la GUI, la normalisation des codes de jumelage, le rejet d'un mauvais code à la poignée de main et un aller-retour chiffré complet sur socket.

## Licence

Distribué sous licence [MIT](LICENSE).

## Icône de l'application

L'icône R_M_X (`assets/`) est appliquée à la fenêtre au lancement (`src/icon.rs` embarque les pixels RGBA), donc visible dans la barre des tâches et le dock :

- **Linux** : icône de fenêtre via le protocole Wayland/X11 (aucune étape supplémentaire).
- **Windows** : en plus de l'icône de fenêtre, l'icône est embarquée dans l'exécutable via `build.rs` + `assets/icon.ico` (`winresource`). `cargo build --release` suffit ; pour la barre des tâches, Windows peut mettre en cache l'icône des exécutables renommés — lancer depuis un chemin stable.
- **macOS** : le dock n'affiche que l'icône d'un bundle `.app`. Empaqueter via `cargo bundle` ou :

```sh
mkdir -p dirtransfer.app/Contents/MacOS
cp target/release/DirTransfert dirtransfer.app/Contents/MacOS/
cp assets/icon-256.png dirtransfer.app/Contents/Resources/AppIcon.png
printf '<?xml version="1.0"?>\n<plist version="1.0"><dict><key>CFBundleExecutable</key><string>DirTransfert</string><key>CFBundleIconFile</key><string>AppIcon</string></dict></plist>' > dirtransfer.app/Contents/Info.plist
```
