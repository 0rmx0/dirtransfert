# dirtransfer

Outil de transfert d'un répertoire complet (et ses sous-répertoires) entre deux PC Windows, conçu pour plusieurs milliers de fichiers et plusieurs dizaines de Go. Zéro dépendance externe : uniquement la bibliothèque standard Rust.

## Fonctionnalités

- **Protocole TCP maison** (`DTX1`) : manifeste binaire, puis transfert fichier par fichier à la demande du récepteur.
- **Intégrité SHA-256** : chaque fichier est haché à la source, vérifié à l'écriture et re-vérifié en fin de transfert. Écriture atomique (fichier temporaire `.dtxpart` puis renommage) : jamais de fichier à moitié écrit.
- **Reprise sur interruption** : si le transfert est coupé (coupure réseau, PC éteint), relancez les deux côtés — les fichiers déjà arrivés avec un checksum valide sont ignorés, seuls les manquants sont retransférés.
- **Barre de progression** : octets transférés, débit Mo/s et pourcentage en temps réel côté récepteur.
- **Sécurité des chemins** : les chemins relatifs sont filtrés (pas de `..`), pas de traversée de répertoire possible.

## Compilation

Sur chaque PC Windows (installez [Rust](https://rustup.rs) si besoin) :

```cmd
cargo build --release
```

Le binaire est `target\release\dirtransfer.exe`. C'est un fichier autonome, aucun runtime requis.

## Utilisation

**1. Sur le PC récepteur** (ouvrez le port 47500 dans le pare-feu Windows) :

```cmd
dirtransfer.exe receive 0.0.0.0:47500 D:\reception
```

**2. Sur le PC émetteur** :

```cmd
dirtransfer.exe send D:\donnees 192.168.1.10:47500
```

Remplacez `192.168.1.10` par l'adresse IP publique du récepteur (ou son nom DNS). Le répertoire est recréé sous `D:\reception\donnees` avec toute son arborescence.

**En cas d'interruption** : relancez simplement `receive` puis `send` — la reprise est automatique.

## Transfert via Internet

Les deux PC n'étant pas sur le même réseau, choisissez l'une de ces options :

1. **VPN entre les deux PC** (recommandé) : par exemple WireGuard ou Tailscale. Ensuite, utilisez l'adresse IP du VPN comme destination.
2. **Redirection de port** sur la box du récepteur : redirigez le port TCP 47500 vers le PC récepteur, et utilisez l'IP publique de la box côté émetteur.
3. Le trafic est un flux TCP brut : sur une ligne ADSL à 1 Mo/s d'upload, comptez ~4,5 h pour 16 Go ; sur du fibre symétrique, ~20-30 min.

## Limites connues

- Transfert séquentiel, un fichier à la fois (fiable et simple, mais n'utilise pas le lien en parallèle si les fichiers sont très petits).
- Pas de chiffrement du flux : pour un transfert sensible via Internet, utilisez un VPN, ou encapsulez dans un tunnel chiffré.
- Un seul transfert à la fois par instance (le récepteur accepte une unique connexion).

## Structure du code

- `src/main.rs` — CLI, logique send/receive, progression, reprise.
- `src/protocol.rs` — manifeste, encodage/décodage des messages, scan du répertoire source.
- `src/sha256.rs` — implémentation SHA-256 en pur Rust (testée contre les vecteurs NIST).

## Tests

```cmd
cargo test
```

Vérifie l'implémentation SHA-256 (vecteurs « abc », vide, et 1 Mo de « a »).
