# Progression V4

## 16 octobre 2026 — V4-1 : premier bring-up réel

Le projet est créé comme workspace
Rust indépendant. RustRover peut l'ouvrir directement avec son `Cargo.toml`.

La contrainte électrique de la console DEBUG est fixée à **1,8 V logique**.
Elle ne doit pas être confondue avec la liaison 3,3 V de la Jupiter 1.

Socle logiciel présent :

- `host/jupiter2ctl` avec moniteur UART `115200 8N1` sans contrôle de flux ;
- serveur TFTP `octet`, RRQ, blocs de 512 octets, ACK et retransmissions ;
- `protocol`, crate `no_std` pour les futures trames de commande ;
- `exec-v4` et `filesystem-v4` repris de la V3, avec scheduler multi-hart,
  états SMP, IPI, SBI, contextes et contrats MMU ;
- image `firmware/bonjour`, volontairement activée seulement avec `--features
  board` tant que le linker et le profil de boot Jupiter 2 ne sont pas fixés ;
- configurations RustRover pour l'information, l'UART et le TFTP.

Validation : `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo
run -p jupiter2ctl -- info` et un smoke TFTP UDP local sont passés. Le test
d'ouverture UART atteint le FTDI FT232R sur `/dev/ttyUSB0` en logique 1,8 V.
Le compte `mich` a été ajouté au groupe Linux `dialout`. Le workspace compte
142 tests passants : 135 pour `exec-v4` et 7 pour `filesystem-v4`.

Bring-up matériel confirmé :

- U-Boot SPL et U-Boot 2022.10 démarrent depuis la NOR `w25q64dw` de 8 MiB ;
- la carte est identifiée comme SpacemiT K3 Pico ITX avec 16 GiB de LPDDR5 ;
- l'UFS contient Bianbu 4.0.7, Linux 6.18.3-generic et le DTB K3 Pico ITX ;
- Linux a confirmé 16 CPU, le RGX `BVNC 36.56.104.183` et le DPU/DisplayPort ;
- l'environnement `/boot/env_k3.txt` a été remplacé temporairement pour arrêter
  le boot avant Linux et obtenir le prompt U-Boot `=>`.

L'environnement original est sauvegardé dans `/boot/env_k3.txt.boot_bak` et la
version intermédiaire dans `/boot/env_k3.txt.bootdelay_bak`. La carte est
laissée au prompt U-Boot. La procédure complète et les lignes de boot sont
consignées dans [`JUPITER2_BRINGUP.md`](JUPITER2_BRINGUP.md).

Ce bring-up valide la console, le bootloader, la mémoire, le stockage, le
réseau et un transfert TFTP réel. Il ne valide pas encore le firmware V4, son
format, son adresse de chargement, le protocole V4 ni son exécution.

Le serveur TFTP est maintenant lancé sur le PC Ryzen 5 en UDP 69, avec une RRQ
réelle validée sur `jupiter2-test.bin`. L'adresse du PC est `192.168.1.131/24`
sur `enp3s0` ; la Jupiter utilise temporairement `192.168.1.132`.

Le lien Ethernet a été validé par U-Boot : `192.168.1.132` pingue
`192.168.1.131`, puis U-Boot a transféré 22 octets à l'adresse
`0x102000000`. Ces adresses restent volatiles et n'ont pas été enregistrées
dans l'environnement de boot.

Le jeu d'assets V3 a été ajouté au dépôt sous `assets/` : 7 fontes, 3 images,
les curseurs et les shaders GLSL/SPIR-V. Ces ressources sont prêtes pour
l'outillage hôte et le futur rendu ; elles ne sont pas encore incluses dans
une image firmware ou MFFS.

Le prochain jalon retenu est le splash matériel sans shaders :
`splashscreen-1920x1080.bgra` est copié dans `tftp-root/`, mesure 8 294 400
octets et sera chargé à `0x102000000`. Le firmware devra journaliser son
initialisation UART, la disponibilité de la DRAM, le chargement du framebuffer
et chaque étape DPU/DisplayPort. Le RGX et les modules SPIR-V restent hors de
ce premier affichage.

L'image `firmware/bonjour` est maintenant compilée pour
`riscv64gc-unknown-none-elf`, liée à `0x140000000` et exportée en binaire plat
par `scripts/build-bonjour.sh`. Elle initialise provisoirement l'UART K3
documenté à `0xD4017000`, écrit `Bonjour Jupiter 2 V4` et lit les extrémités du
framebuffer à `0x102000000`. Le DPU n'est pas encore programmé.

Le 17 octobre, ce firmware a été transféré et exécuté sur la Jupiter 2. U-Boot
a chargé le splash BGRA (`8294400` octets) à `0x102000000`, puis
`bonjour-jupiter2.bin` (`5864` octets) à `0x140000000`. Après `go 0x140000000`,
l'UART a reçu `Bonjour Jupiter 2 V4`, `UART_OK`, `DRAM_OK` et
`SPLASH_RAM_READ`. Le DPU/DisplayPort reste le prochain bloc matériel à
initialiser pour rendre le splash visible.

Une image de diagnostic de 5864 octets a ensuite été transférée à
`0x140000000`. Après `go 0x140000000`, elle a produit `MMIO_PROBE_OK` et les
lectures `DPU1+0x000=0x08040030`, `DPU1+0x340=0`, `DPU1+0x380=0`,
`DPU1+0x3C0=0`, `DP1+0x000=0` et `DP1+0x00C=0x02A1350D`. La signature DPU
correspond à la lecture U-Boot. Cette étape valide la lecture bare-metal des
blocs et fixe DP1 comme sortie à porter. Le firmware reste en lecture seule.

Le 17 octobre, la sonde APMU a été ajoutée au firmware. Après un nouveau
chargement TFTP du splash (`8294400` octets) et de l'image `bonjour`
(`5944` octets), U-Boot a relevé :

```text
0xD4282A3C = 0x00070000
0xD4282A6C = 0x0080401C
0xD4282A70 = 0x01A9C343
0xD4282A74 = 0x00000003
```

Le firmware a ensuite produit les mêmes mots sous les offsets APMU
`0x23C`, `0x26C`, `0x270` et `0x274`, puis `APMU_PROBE_OK`. Cette étape valide
la lecture bare-metal des contrôleurs d'horloge et de reset du chemin DPU1/eDP1.
La sonde est toujours strictement en lecture seule.

L'image suivante est compilée dans `tftp-root/bonjour-jupiter2.bin` (10224
octets). Elle applique en lecture-modification-écriture les gates et resets
DPU1/eDP1 documentés par Linux, avec lecture de retour et les marqueurs
`APMU_ENABLE_BEGIN` puis `APMU_ENABLE_OK`. Aucun registre Saturn, DMMU, RDMA,
cmdlist ou DP n'est encore modifié.

Le watchdog U-Boot a redémarré la carte, ce qui a permis de charger cette
image et le splash une nouvelle fois. L'exécution réelle a produit :

```text
APMU+0x26C_RB=0x0081401D
APMU+0x270_RB=0x01A9C343
APMU+0x274_RB=0x00000003
APMU+0x23C_RB=0x00070000
APMU_ENABLE_OK
```

La transition de `CTRL3` ajoute les gates MCLK et PXCLK attendues, et la
lecture de retour valide l'écriture. Le firmware reste volontairement arrêté
avant la programmation Saturn et DisplayPort.

L'initialisation Saturn minimale a ensuite été ajoutée et exécutée. Après
correction du mapping `CMP1_BASE_ADDR` du pipeline 1, le firmware a publié
`DPU_REG_INIT_OK` avec les lectures de retour DMA top, MMU top et composer :

```text
DPU1+0x1284_RB=0x00000810
DPU1+0x1288_RB=0xFFFFFFFE
DPU1+0x1200_RB=0xFFFF0000
DPU1+0x1208_RB=0x00000008
DPU1+0x120C_RB=0x00000008
DPU1+0x30000_RB=0x00000001
```

Le composer est activé sans couche et le scanout n'est pas encore lancé. La
suite porte sur le RDMA et la table DMMU du splash.

Le timing Saturn du pipeline DP1 a ensuite été préparé et exécuté sur la carte.
Le bloc utilisé est `TMG0` à `DPU1 + 0x51200`, avec `CMP1` déjà confirmé à
`DPU1 + 0x30000`. La séquence UART a publié `TIMING_STAGE_OK` après lecture de
retour des valeurs 1920×1080, porches 88/148, synchros 44/5 et porches verticaux
4/36 en `RGB888`. Le bit de démarrage du timing reste désactivé ; aucun
scanout n'est donc lancé à ce jalon.

L'image de ce jalon configure le plane RDMA0 en `BGRA8888` (format Saturn 7),
avec adresse `0x102000000`, largeur 1920, hauteur 1080 et stride 7680. Cette
préparation a été exécutée sur la carte et a publié
`RDMA_STAGE_OK` avec lecture de retour de l'adresse, du stride, de la taille,
de la boîte et du format. La couche composer reste désactivée. L'image
suivante doit établir le mapping DMMU et la cmdlist avant l'activation du PLL
pixel et du flux DP.

## Suite immédiate

1. porter la table DMMU du plane RDMA0 BGRA8888 ;
2. porter la cmdlist Saturn et la validation de scène ;
3. porter le link training AUX et essayer le scanout du framebuffer ;
4. journaliser `DPU_OK`, `DP_OK` et `SPLASH_OK` sur l'UART ;
5. ajouter le transport de commandes V4 après la présentation matérielle ;
6. seulement après cette preuve, porter les contrats UI/Vulkan de TacticalGlobe
   et les contrats `RenderPlan`/surfaces de V3.

## 17 octobre 2026 — V4-2 : AUX et premier scanout DP

Le routage du switch écran a été pris en compte : l'entrée Jupiter 2 doit être
sélectionnée pour présenter HPD/AUX au connecteur DP1. U-Boot a confirmé HPD
actif et le sink DPCD 1.2, 5,4 Gb/s, deux voies. Une transaction AUX brute
U-Boot avec `AUX_CONFIG=0x0F000009` a réussi.

Le helper bare-metal a ensuite été corrigé pour écrire directement les
registres de commande AUX, notamment `AUX_START` avec la séquence `0` puis
`0x80000000`. DP1 est le DisplayPort externe (`dp-id = 1`) : `ENABLE_EDP` est
laissé à zéro. La séquence réinitialise l'AUX avant la lecture DPCD.

La preuve réelle suivante est passée sur la carte :

```text
AUX_AFTER_DPU_OK
AUX_AFTER_PLL_FAIL
AUX_STAGE_OK
LINK_CR_STATUS=0x00000000
LINK_CR_STATUS=0x00000000
LINK_CR_STATUS=0x00000011
LINK_EQ_STATUS=0x00000077
LINK_TRAIN_OK
TMG0+0x00_ACTIVE_RB=0x00000181
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
MMIO_PROBE_OK
```

Le sink a accepté HBR 2,70 Gb/s sur deux voies. Le MSA DP est configuré pour
1920×1080 RGB888 et le timing Saturn est activé ; le framebuffer BGRA8 reste à
`0x102000000`. `SPLASH_OK` n'est pas encore journalisé : il sera ajouté après
confirmation visuelle du splash sur l'écran relié au switch Jupiter.

Le binaire courant `tftp-root/bonjour-jupiter2.bin` mesure 30 757 octets. La
sonde AUX précoce est intermittente selon l'état laissé par U-Boot ; le reset
AUX dédié rétablit systématiquement les transactions. Les probes
`AUX_EARLY` et `AUX_AFTER_DPU/PLL` restent des instruments de diagnostic et
pourront être retirés après la confirmation visuelle. La suite est la
validation de l'image visible, puis le nettoyage du firmware de bring-up avant
le transport de commandes V4 et le portage TacticalGlobe/V3.

Le premier essai visuel avec l'écran relié au switch n'a affiché aucun splash,
malgré la séquence UART complète `AUX_STAGE_OK`, `LINK_TRAIN_OK`,
`DP_MSA_STAGE_OK` et `SCANOUT_STAGE_OK`. `SPLASH_OK` reste donc en attente de
confirmation à l'écran. Le prochain test utilisera le HP Compaq LA2006x, dont
le mode natif est probablement 1600×900 ; le firmware actuel est encore fixé
à 1920×1080 et devra lire l'EDID ou sélectionner le mode adapté.

## 17 octobre 2026 — diagnostic sans signal et correctif cmdlist

Le HP Compaq LA2006x est maintenant relié directement à la Jupiter 2, mais
l'écran indique encore « pas de signal ». Le link training DP confirmé par
UART ne prouve donc pas que le plane Saturn produit des trames visibles.

Le prochain binaire conserve `PHY_BUSY_BYP` actif après l'entraînement, comme
le pilote Linux. Il corrige aussi la taille de la table DMMU pour couvrir les
1407 pages nécessaires aux 5 760 000 octets du framebuffer BGRA, puis encode
dans l'en-tête cmdlist les événements reload RDMA et composer attendus par
Saturn. La reconstruction est validée (`cargo fmt`, build `bonjour`,
`git diff --check`) ; le binaire TFTP fait 30 781 octets.

Cette version n'a pas encore été exécutée sur la carte : un reset physique est
nécessaire avant le prochain chargement U-Boot.

## 17 octobre 2026 — exécution du correctif cmdlist

Après reset, le HP Compaq LA2006x a de nouveau été détecté par U-Boot en
1600×900, DPCD 1.2, HBR2 et deux voies. U-Boot a transféré le framebuffer
1600×900 de 5 760 000 octets à `0x102000000`, puis le binaire de 30 781 octets
à `0x140000000`.

La trace UART confirme les corrections DMMU et cmdlist, ainsi que le maintien
de `PHY_BUSY_BYP` :

```text
DMMU_TABLE_LAST=0x0010257E
TBU0+0x28_RB=0x0000057F
DMMU_STAGE_OK
CMDLIST_STAGE_OK
CMDLIST_ENABLE_OK
DP1+0x100_POST_TRAIN_RB=0x80060121
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
MMIO_PROBE_OK
```

`SPLASH_OK` reste volontairement en attente de la confirmation visuelle sur le
HP Compaq. `AUX_AFTER_PLL_FAIL` est toujours suivi d'une séquence AUX dédiée
qui aboutit à `AUX_STAGE_OK`.
## 17 octobre 2026 — essai du timing DMT 1600×900@60

L'utilisateur confirme que le HP Compaq LA2006x fonctionne en 1600×900 à
60 Hz. Le firmware ne retenait pas encore le bon minutage : il utilisait
97,5 MHz en CVT reduced blanking. Le prochain essai passe au mode DMT courant
108 MHz avec `HFP/HSYNC/HBP = 24/80/96` et `VFP/VSYNC/VBP = 1/3/96`.
Le PLL DP est aligné sur la table Linux 108 MHz (`fbdiv=117`, `divaux=13`).

La compilation est validée et le binaire TFTP fait 30 781 octets. La trace
UART et l'affichage restent à vérifier après reset et exécution sur la carte.

## 17 octobre 2026 — exécution du mode DMT 1600×900@60

Le reset et le chargement automatique ont réussi. U-Boot a transféré le
framebuffer 1600×900 de 5 760 000 octets et le firmware de 30 781 octets.
La nouvelle image a utilisé le pixel clock 108 MHz et le timing DMT
`HFP/HSYNC/HBP = 24/80/96`, `VFP/VSYNC/VBP = 1/3/96`.

La trace UART confirme :

```text
DMMU_STAGE_OK
CMDLIST_ENABLE_OK
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
MMIO_PROBE_OK
```

Le chemin DP est actif côté contrôleur. La validation visuelle du splash sur le
HP Compaq LA2006x reste le dernier contrôle de ce jalon ; `SPLASH_OK` n'est pas
encore ajouté.

## 17 octobre 2026 — précision sur la chaîne vidéo

La sortie physique utilisée est USB-C vers HDMI via un adaptateur. `DP1` dans
les traces désigne le transmetteur DisplayPort côté Jupiter, avant conversion
par l'adaptateur. `LINK_TRAIN_OK` valide cette liaison source/adaptateur ; la
détection HDMI par le HP reste à valider séparément. Le prochain test de mire
interne DP sert précisément à isoler cette chaîne du framebuffer Saturn.

## 17 octobre 2026 — HBR2 et lien maintenu après activation vidéo

Le sink a annoncé HBR2 (`DPCD_RATE=0x14`) sur deux voies. Le firmware a été
reconstruit avec la PLL cœur 5,4 Gb/s et les valeurs MSA/TU HBR2. La trace
UART confirme :

```text
DP1+0x100_RB=0x80060122
LINK_TRAIN_OK
DP1+0x200_VIDEO_RB=0x10400000
DP1+0x238_COLORBAR_RB=0x00000001
DP1+0x28_CLKSEL_RB=0x00000000
LINK_POST_STATUS=0x00000077
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
```

Le lien reste valide après activation du flux, mais le HP Compaq LA2006x
affiche toujours « pas de signal » avec la chaîne USB‑C/HDMI. Le prochain
diagnostic doit isoler l'adaptateur et le timing HDMI ; le splash n'est pas
encore validé visuellement.

## 17 octobre 2026 — référence Bianbu et passage V4 en RBR

Le démarrage Bianbu avec le même adaptateur USB‑C/HDMI a fourni la référence
réelle du pilote Linux :

```text
DP: Attempting Config: R=1620000, L=2
Setting PLL to Rate 1620000 kHz, Pclk 108000 kHz
DP: Training successful for R:1620000 L:2.
MSA: 1600x900, Rate:1620000 kHz, Lanes:2, BPP:24, TU:64.0
```

Le firmware V4 a été aligné sur ce mode RBR et exécuté après transfert TFTP.
La trace confirme :

```text
DP1+0x180_RB=0x873002B0
DP1+0x100_POST_TRAIN_RB=0x80060120
LINK_TRAIN_OK
DP1+0x200_VIDEO_RB=0x10400000
LINK_POST_STATUS=0x00000077
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
```

Le lien et le flux DP sont maintenant configurés comme dans Bianbu. La
validation visuelle du splash reste à inscrire avec `SPLASH_OK`.

## 17 octobre 2026 — état du boot et arrêt

Une écoute UART après reset a confirmé que le binaire V4 est bien atteint par
U-Boot et s'arrête volontairement sur son point d'attente :

```text
[   9.462] V4_BOOT_HALTED
=>
```

Ce test n'a donc pas lancé la séquence DPU/DP et ne peut pas produire de
signal vidéo. La commande U-Boot `poweroff` a été envoyée, mais la carte
répondait encore au ping ; la mise hors tension doit être faite par
l'interrupteur ou en débranchant son alimentation.

## 17 octobre 2026 — exécution V4 après réinitialisation

Après le reset, U-Boot avait perdu son adresse réseau. Les variables ont été
renseignées puis les deux transferts TFTP ont réussi :

```text
ipaddr  = 192.168.1.132
serverip = 192.168.1.131
splashscreen-1600x900.bgra : 5760000 octets à 0x102000000
bonjour-jupiter2.bin        : 30861 octets à 0x140000000
```

Le lancement `go 0x140000000` a ensuite produit `UART_OK`, `TIMING_STAGE_OK`,
`RDMA_STAGE_OK`, `DMMU_STAGE_OK`, `CMDLIST_ENABLE_OK`, `LINK_TRAIN_OK`,
`DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `MMIO_PROBE_OK`. Le firmware V4
s'exécute donc bien sur la carte ; `SPLASH_OK` reste réservé à la confirmation
visuelle sur le moniteur.
