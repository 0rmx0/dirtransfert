# Dépôts de distribution (APT Ubuntu / winget Windows)

## Ubuntu — dépôt APT via GitHub Pages

Le workflow `.github/workflows/apt-repo.yml` publie automatiquement tous les `.deb`
issus des releases GitHub dans une branche `gh-pages` servie par GitHub Pages.

### Prérequis (une seule fois)

1. Générer une clé de signature de dépôt :

   ```bash
   gpg --full-generate-key   # RSA 4096, usage sign
   gpg --export-secret-keys -a "Votre Nom" > apt-private.key
   ```

2. Ajouter le contenu de `apt-private.key` dans le secret GitHub
   **`APT_GPG_KEY`** (Settings → Secrets and variables → Actions).

3. Activer GitHub Pages sur la branche `gh-pages` :
   Settings → Pages → Source: *Deploy from a branch* → Branch: `gh-pages`, `/ (root)`.

### Publication

Le workflow se déclenche automatiquement après chaque workflow *Release* réussi,
ou manuellement (onglet Actions → *APT repo* → *Run workflow*).

### Installation côté utilisateur

```bash
curl -fsSL https://0rmx0.github.io/dirtransfert/dirtransfert.asc \
  | sudo gpg --dearmor -o /usr/share/keyrings/dirtransfert.gpg
echo "deb [arch=amd64,arm64 signed-by=/usr/share/keyrings/dirtransfert.gpg] https://0rmx0.github.io/dirtransfert stable main" \
  | sudo tee /etc/apt/sources.list.d/dirtransfert.list
sudo apt update && sudo apt install dirtransfert
```

Les mises à jour arrivent ensuite via `sudo apt upgrade`.

## Windows — winget

Le workflow `.github/workflows/release.yml` construit un `.msi`
(`cargo wix`, template `wix/main.wxs`) publié comme asset de release.

### Soumission initiale (une seule fois)

Les manifests de référence sont dans `packaging/winget/` :

1. Récupérer le SHA-256 du MSI publié :
   `sha256sum DirTransfert-0.3.1-x64.msi`
2. Remplacer `REPLACE_WITH_SHA256_OF_MSI` dans
   `packaging/winget/0rmx0.DirTransfert.installer.yaml`.
3. Créer le fork + PR vers [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) :

   ```powershell
   wingetcreate submit
   ```

   ou copier les 3 fichiers YAML dans
   `manifests/0/0rmx0/DirTransfert/0.3.1/` du fork et ouvrir un PR.

Une fois fusionné : `winget install DirTransfert`.

### Mises à jour suivantes

Soit via le workflow *Winget* (onglet Actions → *Winget* → *Run workflow*,
avec le numéro de version et l'option *submit* cochée — nécessite un PAT
`WINGET_GH_TOKEN` avec les droits fork/PR), soit manuellement :

```powershell
wingetcreate update 0rmx0.DirTransfert `
  --version 0.4.0 `
  --url https://github.com/0rmx0/dirtransfert/releases/download/v0.4.0/DirTransfert-0.4.0-x64.msi `
  --submit
```

Les utilisateurs reçoivent ensuite la MAJ via `winget upgrade`.
