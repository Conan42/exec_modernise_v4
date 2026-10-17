# Exec Modernisé V4

V4 est le nouveau projet de bring-up pour la Milk-V Jupiter 2. Il reprend les
contrats utiles de V1, V2 et V3, et prendra TacticalGlobe comme référence pour
l'organisation de l'UI egui et du rendu Vulkan côté hôte et côté outil de
validation.

> Projet expérimental en cours de publication open source. Le code de
> validation hôte, les contrats portables et le firmware de bring-up sont
> séparés des composants graphiques encore à porter.

## Documentation

- [`ARCHITECTURE.md`](ARCHITECTURE.md) — composants, flux et diagrammes Mermaid ;
- [`HISTORY.md`](HISTORY.md) — histoire V1 → V4 et archive des sessions ;
- [`docs/AMIGAOS_HISTORY.md`](docs/AMIGAOS_HISTORY.md) — héritage historique AmigaOS ;
- [`JUPITER2_BRINGUP.md`](JUPITER2_BRINGUP.md) — procédure et traces matérielles ;
- [`PROGRESS.md`](PROGRESS.md) — journal des jalons et décisions ;
- [`CONTRIBUTING.md`](CONTRIBUTING.md) — règles de contribution et validation ;
- [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md) — provenance et licences des assets.
- [`docs/OPEN_SOURCE.md`](docs/OPEN_SOURCE.md) — checklist de publication GitHub.

## État actuel

Le firmware `bonjour` est transféré par TFTP et exécuté sur la Jupiter 2. Les
étapes UART `DMMU_STAGE_OK`, `CMDLIST_ENABLE_OK`, `LINK_TRAIN_OK`,
`DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `MMIO_PROBE_OK` sont confirmées. Le
HP Compaq LA2006x reste sans signal via l'adaptateur USB-C/HDMI ;
`SPLASH_OK` n'est pas déclaré. Le prochain diagnostic devra isoler la sortie
USB-C/HDMI et la synchronisation finale du moniteur.

Le premier jalon est volontairement limité à la communication avec la carte :

- UART DEBUG : `/dev/ttyUSB0`, **logique 1,8 V**, `115200 8N1`, sans contrôle
  de flux ;
- TFTP : serveur de fichiers pour les requêtes RRQ du bootloader ;
- protocole partagé `no_std` pour les futures commandes binaires ;
- cœur `exec-v4` repris de V3 avec ordonnanceur par hart, SMP, IPI, SBI,
  contextes et gestion mémoire portable ;
- image firmware minimale séparée, activée seulement lorsque le profil de boot
  Jupiter 2 et son adresse de chargement sont confirmés.

## État du bring-up matériel

Le premier démarrage réel de la carte est confirmé. La console utilise un FTDI
FT232R sur `/dev/ttyUSB0`, en logique 1,8 V, `115200 8N1`, sans contrôle de
flux. Le compte de travail doit appartenir au groupe Linux `dialout` pour
ouvrir directement le port.

La carte est une **SpacemiT K3 Pico ITX** avec 16 GiB de LPDDR5. U-Boot SPL et
U-Boot 2022.10 démarrent depuis la NOR `w25q64dw`, puis le système Linux
Bianbu 4.0.7 est chargé depuis l'UFS. Linux 6.18.3-generic, le DTB
`k3-pico-itx.dtb`, le RGX et le DPU/DisplayPort ont été vus dans les logs UART.

Pour permettre l'inspection du boot sans connaître la touche d'interruption,
le fichier `/boot/env_k3.txt` a été remplacé temporairement sur la carte par :

```text
bootdelay=5
bootcmd=echo V4_BOOT_HALTED
```

La copie originale est `/boot/env_k3.txt.boot_bak` et la copie intermédiaire,
après ajout de `bootdelay`, est `/boot/env_k3.txt.bootdelay_bak`. Cette
configuration a servi à inspecter U-Boot ; les essais V4 ont ensuite été
chargés manuellement depuis le prompt. Le détail des commandes et des logs est dans
[`JUPITER2_BRINGUP.md`](JUPITER2_BRINGUP.md).

Le chemin Ethernet et le TFTP sont validés sur la carte : U-Boot a chargé le
splash BGRA puis exécuté `firmware/bonjour` à `0x140000000`. Les logs `UART_OK`,
`DRAM_OK`, `SPLASH_RAM_READ`, `TIMING_STAGE_OK`, `RDMA_STAGE_OK`,
`DMMU_STAGE_OK`, `CMDLIST_ENABLE_OK`, `LINK_TRAIN_OK`, `DP_MSA_STAGE_OK` et
`SCANOUT_STAGE_OK` sont confirmés. La dernière étape manquante est la
confirmation visuelle du signal au bout de la chaîne USB-C/HDMI.

La sortie physique retenue par le DTB Pico-ITX est DP1/eDP1 : DPU1 à
`0xC0440000` et DP1 à `0xCAC88000`. Une image de diagnostic V4 a lu ces
fenêtres avec succès et obtenu la signature DPU `0x08040030`. Le prochain
portage suit le pilote Linux : power-domain, horloges, resets, DMMU, cmdlist
RDMA, link training AUX puis flux vidéo.

La première image de test s'appelle `firmware/bonjour`. Elle se construit avec
[`scripts/build-bonjour.sh`](scripts/build-bonjour.sh) et produit
`tftp-root/bonjour-jupiter2.bin`. Elle journalise d'abord sur l'UART avant que
le chemin DPU/DisplayPort ne soit ajouté.

Le projet est indépendant du dépôt TacticalGlobe et ne modifie pas son pipeline
de relief. Il ne déclare pas encore le backend RGX/Vulkan ou V2D/DPU matériel :
les documents V3 indiquent que ces blocs nécessitent d'abord une séquence
clocks, reset, power-domain, IOMMU, DMA et IRQ validée sur la carte.

Le répertoire [`assets/`](assets/) reprend les ressources UI et graphiques de
V3 : fontes, images BGRA/JPEG et shaders GLSL/SPIR-V. Elles sont réservées à
l'outillage hôte et au futur rendu V4 ; elles ne font pas partie de l'image
firmware ni du transfert TFTP Jupiter 2.

> **Attention électrique — UART DEBUG Jupiter 2 : 1,8 V.** GND doit être
> commun et TXD/RXD croisés. Ne pas brancher cette liaison avec un adaptateur
> 3,3 V : la documentation de la Jupiter 1 en 3,3 V ne s'applique pas à cette
> connexion.

## Ouvrir dans RustRover

Ouvrir le fichier `Cargo.toml` de ce répertoire. RustRover détecte le workspace
et le binaire `jupiter2ctl` automatiquement. Les configurations `Info V4`,
`Monitor UART Jupiter 2` et `Serveur TFTP Jupiter 2` sont fournies dans
`.idea/runConfigurations/`.

## Premier essai

```bash
cargo run -p jupiter2ctl -- info
cargo run -p jupiter2ctl -- monitor --serial /dev/ttyUSB0
cargo run -p jupiter2ctl -- tftp-serve --root tftp-root
```

Le port TFTP 69 nécessite généralement une capacité réseau ou un lancement
privilégié. Pour un test local sans privilège, utiliser un port supérieur à
1024 ; le bootloader qui attend le port standard doit, lui, recevoir le service
sur UDP 69.

L'état du travail et les preuves de validation sont suivis dans
[`PROGRESS.md`](PROGRESS.md).

## Open source

Le code Rust du workspace est publié sous MIT. Les fichiers importés dans
`assets/` conservent leurs licences propres, détaillées dans
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md). Avant toute redistribution
d'une image complète, vérifier la provenance des images et des shaders ajoutés
au dépôt.
