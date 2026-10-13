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

**1. Sur le PC récepteur**, dans une console **ouvertes en tant qu'administrateur** (clic droit → « Exécuter en tant qu'administrateur ») :

```cmd
dirtransfer.exe receive 0.0.0.0:47500 D:\reception --upnp --open-fw --close-fw
```

L'outil ouvre alors automatiquement :
- la **règle entrante dans le pare-feu Windows** (`--open-fw`, règle nommée `dirtransfer`, port TCP 47500) ;
- la **redirection de port sur la box** (`--upnp`, via UPnP-IGD : découverte SSDP de la box, `AddPortMapping`, récupération de l'IP publique).

Il affiche ensuite la **commande `send` prête à copier** avec l'IP publique de la box.

**2. Sur le PC émetteur** :

```cmd
dirtransfer.exe send D:\donnees 82.65.12.34:47500
```

Le répertoire est recréé sous `D:\reception\donnees` avec toute son arborescence. À la fin du transfert, avec `--close-fw`, la règle pare-feu et la redirection UPnP sont **automatiquement fermées**.

**En cas d'interruption** : relancez simplement `receive` puis `send` — la reprise est automatique.

### Options du récepteur

| Option | Effet |
|---|---|
| `--upnp` | Redirection de port automatique sur la box (UPnP-IGD) |
| `--open-fw` | Règle entrante automatique dans le pare-feu Windows (exige une console administrateur) |
| `--close-fw` | Ferme pare-feu + redirection UPnP après le transfert |

Si une étape échoue (box sans UPnP, console non administrateur), l'outil l'affiche clairement et continue — il indique alors quoi ouvrir manuellement.

## Transfert via Internet

1. **UPnP automatique** : la méthode par défaut ci-dessus fonctionne si la box a l'UPnP activé (cas de la quasi-totalité des box françaises : Livebox, Freebox, SFR Box, Bbox).
2. **VPN entre les deux PC** si l'UPnP est désactivé ou bloqué par le FAI (certains bloquent les ports entrants, notamment en CGNAT/4G) : WireGuard ou Tailscale, puis utilisez l'adresse IP du VPN comme destination.
3. Le trafic est un flux TCP brut : sur une ligne ADSL à 1 Mo/s d'upload, comptez ~4,5 h pour 16 Go ; sur du fibre symétrique, ~20-30 min.

**Note CGNAT** : si l'IP publique affichée commence par `100.64.` à `100.127.`, vous êtes derrière un CGNAT opérateur — l'UPnP ne suffira pas, utilisez le VPN.

## Limites connues

- Transfert séquentiel, un fichier à la fois (fiable et simple, mais n'utilise pas le lien en parallèle si les fichiers sont très petits).
- Pas de chiffrement du flux : pour un transfert sensible via Internet, utilisez un VPN, ou encapsulez dans un tunnel chiffré.
- Un seul transfert à la fois par instance (le récepteur accepte une unique connexion).

## Structure du code

- `src/main.rs` — CLI, logique send/receive, progression, reprise.
- `src/protocol.rs` — manifeste, encodage/décodage des messages, scan du répertoire source.
- `src/sha256.rs` — implémentation SHA-256 en pur Rust (testée contre les vecteurs NIST).
- `src/upnp.rs` — client UPnP-IGD en pur Rust : découverte SSDP, description XML, appels SOAP `AddPortMapping`/`GetExternalIPAddress`/`DeletePortMapping`.
- `src/firewall.rs` — règles `netsh advfirewall` pour le pare-feu Windows (règle entrante `dirtransfer`).

## Tests

```cmd
cargo test
```

Vérifie l'implémentation SHA-256 (vecteurs « abc », vide, et 1 Mo de « a »).
