# filesystem-v4

Cette crate porte la validation portable des images ELF64 RISC-V et la lecture
bornée des catalogues ExecFS/MFFS. `parse_elf64()` vérifie la classe,
l'encodage, `ET_EXEC`, la machine RISC-V, les `PT_LOAD`, les bornes
fichier/mémoire, l'alignement, W^X, les recouvrements et le point d'entrée
exécutable.

`MffsImage::parse()` valide l'en-tête et le catalogue MFFS V1 ; `lookup()`
contrôle les noms, les bornes et le checksum FNV-1a avant de rendre un payload.
La crate reste sans allocation et sans accès disque : elle ne monte pas encore
de volume, ne calcule pas d'identité de provenance persistante, ne charge pas
les pages physiques et ne modifie pas les tables Sv39. Le chargeur logique qui
réserve et remplit ces pages est volontairement porté par le crate Exec.

Le crate est membre du workspace sous le nom `filesystem-v4` ; sa validation
est exécutée par `cargo test --workspace`.

L'outil hôte `mffs-pack` valide un ELF APP RISC-V puis construit une image
MFFS V1 à entrée unique. `build-uilab.sh` l'utilise pour produire
`UILab.app.mffs` ; cet outil n'est pas embarqué dans le kernel.

## État au 5 septembre 2026

Le chemin de démarrage QEMU utilise cette validation avant la création du
processus UILab. Le chargement est donc séparé en trois étapes :

```text
parse ELF RISC-V → lookup/checksum MFFS → allocation et copie par Exec
```

Les tests couvrent les en-têtes invalides, les bornes, les recouvrements, la
protection W^X, le point d’entrée et les checksums MFFS. Le montage persistant,
la provenance et l’écriture sur volume restent hors de ce crate.

Le titre UTF-8 de UILab est transporté dans l'ELF/MFFS comme donnée applicative
bornée ; filesystem ne l'interprète pas et ne connaît ni Graphics ni le backend
GPU.
