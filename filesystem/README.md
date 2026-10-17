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

## Rapport avec le bring-up Jupiter 2

Le premier bring-up matériel a validé la console U-Boot, la mémoire LPDDR5 et
le stockage UFS de la SpacemiT K3 Pico ITX. Il n'a pas encore chargé une image
V4 : `filesystem-v4` reste donc une validation portable exécutée sur l'hôte et
dans les tests du workspace. Aucun chemin ne relie encore cette crate au
chargeur U-Boot, au TFTP ou à un volume UFS réel.

L'intégration carte attend la validation préalable de l'adresse de chargement,
du linker et du format d'image `firmware/bonjour`. La progression et les
commandes de restauration de l'environnement de boot sont documentées dans
[`../JUPITER2_BRINGUP.md`](../JUPITER2_BRINGUP.md).

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
