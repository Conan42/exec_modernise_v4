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

## 17 octobre 2026 — activation explicite du plane Saturn

La comparaison avec le pilote Linux a révélé que le firmware ne vérifiait pas
la couche composite après `SW_START`. `CMP1+0x38` était initialisé à zéro,
alors que le commit Linux programme `layer00_ctrl=0x0000FF03` pour activer
RDMA0 dans le compositeur. Le firmware écrit désormais cette valeur et ajoute
le jalon `DPU_LAYER_STAGE_OK`. L'image reconstruite mesure `31125` octets;
elle doit encore être exécutée après reset et vérifiée sur le moniteur.

## 17 octobre 2026 — correction de l'activation de scène DPU1

Une première exécution du binaire avec l'activation de `DPU1+0x380` bloquait
à `DPU_SCENE_STAGE_BEGIN`. Trois appels `write_field()` utilisaient encore les
bornes dans l'ordre `low, high`, contrairement au contrat `high, low` de la
fonction. Les champs scène `9:0`, `7:2` et `13:8` ont été corrigés.

La nouvelle exécution atteint `DPU_SCENE_STAGE_OK` avec les readbacks
`DPU1+0x380=0x01040001` et `DPU1+0x38C=0x00000821`. Elle atteint également
`LINK_TRAIN_OK` et confirme `TMG0+0x00=0x00000184`. Le registre
`DPU1+0x390_SW_START` revient à zéro après écriture, ce qui correspond à un
strobe auto-effacé ; le firmware accepte désormais ce readback pour cette
commande. L'image de test de `31053` octets a été compilée et placée dans
`tftp-root/`. Il reste à l'exécuter après reset et à confirmer le signal du
moniteur avant de publier `SPLASH_OK`.

## 17 octobre 2026 — trace complète après correction du strobe DPU1

Après reset, le binaire de `31053` octets a été transféré et exécuté sur la
Jupiter 2. La trace UART atteint désormais `DPU_SCENE_STAGE_OK`,
`LINK_TRAIN_OK`, `DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `MMIO_PROBE_OK`.
Le readback `DPU1+0x390_SW_START_RB=0x00000000` est accepté comme état normal
du strobe auto-effacé. Le chemin logiciel et matériel jusqu'à l'activation du
scanout est confirmé; `SPLASH_OK` attend encore l'observation du moniteur.

## 17 octobre 2026 — flux DP normal et diagnostic du scanout

La mire interne DP a été désactivée (`DP1+0x238=0`) afin de tester le vrai
framebuffer BGRA à travers RDMA0, Saturn et le compositeur. Cette configuration
reproduit le pilote Linux, qui met `VID_BIST_EN` à zéro avant d'activer le flux
vidéo. Le format TMG RGB888 et le flux DP normal restent actifs.

La trace ajoute les readbacks du statut IRQ online de DPU1 (`+0x70C` et
`+0x730`) et du bloc debug RDMA0 (`+0xAC`, `+0xB0`, `+0xB4`, `+0xBC` à
`+0xCC`). Cela permettra de distinguer une absence de requêtes mémoire, un
underrun DPU et un flux DP sans signal.

La vérification locale est passée : formatage Rust, `cargo test --workspace`
(142 tests), `git diff --check` et compilation firmware. La nouvelle image
`tftp-root/bonjour-jupiter2.bin` mesure `35461` octets. Le port UART est
accessible et TFTP écoute sur UDP 69; l'écoute UART réalisée sans reset n'a
produit aucune nouvelle trace, ce qui laisse la carte dans la boucle `WFI` de
l'exécution précédente. Le test matériel doit donc commencer par un reset,
puis le chargement TFTP de cette image et son lancement par `go`.

## 17 octobre 2026 — lancement V4 et validation du chemin scanout

Après reset, U-Boot a reconnu le HP Compaq LA2006x en `1600x900` et s'est
arrêté sur `V4_BOOT_HALTED`. Le transfert direct du splash de `5760000` octets
perdait des ACK à des blocs variables malgré le serveur TFTP renforcé. Le
fichier a été découpé en 88 morceaux de 64 KiB et chargé successivement à
partir de `0x102000000`; les `5760000` octets ont été transférés correctement.

Le binaire V4 de `35461` octets a ensuite été lancé à `0x140000000`. La trace
atteint `LINK_TRAIN_OK`, `DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et
`SCANOUT_DIAG_OK`. Les diagnostics indiquent `DPU1+0x730_IRQ_STATUS=0` et
`RDMA0+0xB4_DBG_STATUS=0`, sans underrun DPU ni erreur RDMA détectée. Le
framebuffer BGRA est donc maintenant engagé dans le flux DP normal;
`SPLASH_OK` attend toujours la confirmation visuelle.

## 17 octobre 2026 — réapplication du framing DP après reset

La comparaison avec `spacemit_inno_dp.c` a identifié une omission dans la
séquence V4 : le pilote constructeur applique `ENHANCE_FRAMING_EN=1` après la
sortie des resets DP. V4 le configurait avant `DP_SOFT_RESET`, ce qui pouvait
laisser le bit 30 effacé tout en conservant un link training valide.

Le firmware réapplique maintenant, après le reset DP, les champs
`SCRAMBLER_DISABLE=0`, `ENHANCE_FRAMING_EN=1`, `DEFAULT_FAST_LINK_TRAIN=0`
et `SCALE_DOWN_MODE=0`. Il journalise aussi les lectures de
`DP1+0x018`, du statut HPD `DP1+0x088` et des interruptions générales après
l'activation du flux. Le nouveau binaire est construit dans
`tftp-root/bonjour-jupiter2.bin` et mesure `35597` octets.

Les vérifications locales `cargo fmt --all -- --check` et `git diff --check`
sont passées. Une écoute UART sans reset n'a reçu aucune nouvelle trace : la
Jupiter est encore dans la boucle `WFI` de l'image précédente. Le prochain
essai matériel doit donc commencer par un reset, puis les mêmes transferts
TFTP et `go 0x140000000`. `SPLASH_OK` reste réservé à la confirmation visuelle.

## 17 octobre 2026 — exécution du correctif DP post-reset

Après reset, U-Boot a de nouveau détecté le HP Compaq LA2006x en `1600x900`,
et les 88 morceaux du framebuffer ont été chargés à partir de `0x102000000`.
Le nouveau binaire de `35597` octets a été transféré à `0x140000000` puis
exécuté.

La trace UART confirme que le champ de framing reste actif après les resets :

```text
DP1+0x018_CORE_RB=0x40000000
DP1+0x018_CORE_ACTIVE_RB=0x40000000
DP1+0x088_HPD_RB=0x27800000
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SCANOUT_DIAG_OK
MMIO_PROBE_OK
```

Le HPD est présent, le link training reste valide après l'activation du flux,
et aucun underrun DPU/RDMA n'est signalé. La validation visuelle du splash
reste le seul point à confirmer sur le moniteur.

## 17 octobre 2026 — pause de fin de session

Après cette exécution, le HP Compaq LA2006x reste sans signal malgré la trace
complète côté Jupiter : HPD présent, framing DP actif, link training réussi,
MSA configuré et aucun événement d'underrun DPU/RDMA. Le framebuffer BGRA est
donc engagé dans le chemin normal, mais l'image n'est pas confirmée côté
moniteur.

La prochaine session commencera par l'activation temporaire de la mire interne
du transmetteur DP (`VID_BIST_EN=1`). Si la mire apparaît, le problème restant
sera limité au chemin Saturn/RDMA/framebuffer ; sinon, il faudra poursuivre sur
le flux DP ou la conversion USB-C/HDMI. `SPLASH_OK` reste volontairement absent.

## 18 octobre 2026 — test d'un framebuffer rouge minimal

Pour simplifier la preuve d'affichage, le splash a été remplacé par une surface
BGRA8 uniforme rouge (`B=0x00`, `G=0x00`, `R=0xFF`, `A=0xFF`), en 1600×900,
chargée par 88 morceaux de 64 KiB. Le firmware laisse la mire DP interne
désactivée (`DP1+0x238=0`) et utilise le flux normal Saturn/RDMA.

Le test a été transféré et exécuté avec succès jusqu'au scanout :

```text
RED_FRAMEBUFFER_TEST_BEGIN
DP1+0x238_COLORBAR_RB=0x00000000
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SCANOUT_DIAG_OK
MMIO_PROBE_OK
```

Le binaire mesure `35621` octets. Le moniteur doit maintenant afficher une
surface rouge uniforme ; `SPLASH_OK` reste en attente de confirmation visuelle.

## 18 octobre 2026 — mire interne DP et entrée DVI

La surface rouge n'ayant pas produit l'image attendue, le firmware a été
basculé sur la mire interne du transmetteur DP. Cette mire ne lit ni le
framebuffer ni Saturn/RDMA :

```text
DP_COLORBAR_TEST_BEGIN
DP1+0x238_COLORBAR_RB=0x00000001
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SCANOUT_DIAG_OK
MMIO_PROBE_OK
```

Le contrôleur DP émet donc maintenant son propre motif de test. La sortie
physique reste du DisplayPort Alt Mode sur USB-C, converti par l'adaptateur en
HDMI. Une entrée DVI du HP Compaq ne peut recevoir ce signal qu'au moyen d'une
liaison HDMI vers DVI-D compatible. Si la mire reste absente sur DVI, la suite
doit vérifier cette conversion et la sélection d'entrée du moniteur avant de
reprendre Saturn/RDMA.

La mire interne a ensuite été réellement chargée après configuration réseau
U-Boot et transfert du binaire de `35621` octets. La trace confirme
`DP_COLORBAR_TEST_BEGIN`, `DP1+0x238_COLORBAR_RB=0x00000001`,
`LINK_TRAIN_OK`, `DP_MSA_STAGE_OK` et `SCANOUT_DIAG_OK`. Le moniteur reste
cependant sans signal sur l'entrée DVI.

L'utilisateur observe maintenant un écran bleu avec cette mire. C'est la
première confirmation visuelle du chemin complet Jupiter 2 → DisplayPort
Alt Mode USB-C → adaptateur HDMI → liaison DVI → HP Compaq LA2006x. Le DP et
la conversion physique sont donc validés ; le prochain travail revient au
chemin Saturn/RDMA et à l'interprétation du framebuffer BGRA, qui affichait
précédemment du noir au lieu du rouge.

## 18 octobre 2026 — cycle framebuffer bleu rouge vert

La mire interne a été remplacée par un cycle réel du framebuffer BGRA. Le
firmware écrit directement dans la surface 1600×900 puis maintient chaque
couleur assez longtemps pour l'observation :

```text
RGB_CYCLE_TEST_BEGIN
RGB_CYCLE_BLUE
RGB_CYCLE_RED
RGB_CYCLE_GREEN
RGB_CYCLE_DONE
```

La trace complète conserve `LINK_TRAIN_OK`, `DP_MSA_STAGE_OK`,
`SCANOUT_DIAG_OK` et `MMIO_PROBE_OK`. La validation visuelle attendue est une
succession bleu, rouge, vert ; ce test vérifie enfin le chemin Saturn/RDMA en
plus du transmetteur DP.

## 18 octobre 2026 — mire compositeur sans framebuffer

Le cycle framebuffer n'a produit aucun signal, alors que la mire DP interne a
affiché du bleu. `firmware/bonjour` active maintenant `SATURN_SOLID_TEST` :
`CMP1/layer00` génère directement une couleur pleine RGB 10 bits et le canal
RDMA est désactivé. Le firmware cycle bleu, rouge et vert par registres Saturn
après l'activation du scanout.

Les vérifications locales passent (`cargo fmt`, compilation RISC-V, 142 tests
workspace et `git diff --check`). Le binaire a été généré dans
`tftp-root/bonjour-jupiter2.bin` et mesure 36 365 octets. Le test matériel
reste à exécuter après reset et doit être jugé par la trace UART et l'image
réellement visible.

## 18 octobre 2026 — exécution de la mire Saturn solide

La nouvelle image a été transférée par TFTP et exécutée après reset. La trace
UART confirme que le canal RDMA est désactivé (`DPU1+0x380_RB=0x01040000`), que
`CMP1/layer00` est actif en couleur pleine (`CMP1+0x38=0x0000FF09`) et que le
firmware a accepté les trois mises à jour :

```text
RGB_CYCLE_SOLID_BLUE_OK
RGB_CYCLE_SOLID_RED_OK
RGB_CYCLE_SOLID_GREEN_OK
```

Le chemin logiciel du compositeur est donc validé sans lecture framebuffer.
L'observation réelle du moniteur doit encore déterminer si cette mire est
visible et permettre de localiser la panne restante si l'écran reste muet.

## 18 octobre 2026 — reproduction du bleu DP

La mire interne DP a été réactivée dans le firmware et maintenue en continu.
La trace UART confirme `DP1+0x238_COLORBAR_RB=0x00000001`, `LINK_TRAIN_OK`,
`DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `DP_COLORBAR_BLUE_HOLD`. L'écran
affiche de nouveau le bleu.

Cette répétition valide la chaîne physique et le transmetteur DP. Elle montre
également que l'absence d'image avec Saturn est indépendante du câble, de
l'adaptateur USB-C/HDMI, de l'entrée DVI et du moniteur. Le prochain test doit
revenir au compositeur Saturn en conservant cette référence DP connue.

## 18 octobre 2026 — transfert du splash

`splashscreen-1600x900.bgra` a été chargé avec succès par TFTP à l'adresse
DRAM `0x102000000` (`5 760 000` octets). Le firmware de référence DP bleu est
encore actif ; ce transfert valide la présence du splash en mémoire mais ne
constitue pas une preuve d'affichage.

## 18 octobre 2026 — flux Saturn/RDMA relancé

Le splash présent à `0x102000000` a été conservé, puis le firmware a été
reconstruit sans mire DP et sans couche solide. L'exécution réelle confirme
`DPU1+0x380_RB=0x01040001`, `CMP1+0x38_ACTIVE_RB=0x0000FF03`,
`DP1+0x238_COLORBAR_RB=0x00000000` et le cycle `RGB_CYCLE_BLUE`,
`RGB_CYCLE_RED`, `RGB_CYCLE_GREEN`, `RGB_CYCLE_DONE`. L'affichage visible
reste à confirmer.

## 18 octobre 2026 — échec d'affichage Saturn documenté

Le dernier essai a utilisé le fond rouge du compositeur `CMP1` avec la mire DP
désactivée. Le firmware a publié `CMP_BACKGROUND_RED_HOLD` après les étapes
de configuration DPU/DP, mais l'écran est resté noir. Le résultat est donc un
échec visuel du flux Saturn/compositeur, malgré les readbacks UART et
`SCANOUT_STAGE_OK`.

Les contrôles de séparation sont concluants : Bianbu affiche son écran de
connexion avec le même écran HP Compaq LA2006x et le même adaptateur
USB-C/HDMI, le fond rouge produit directement par TMG a été visible et la
mire DP interne bleue a été reproduite. La liaison physique, l'adaptateur, le
moniteur et le transmetteur DP ne sont donc plus les hypothèses prioritaires.

À ce point, `SCANOUT_STAGE_OK` signifie seulement que la séquence de registres
a été exécutée et relue ; il ne vaut pas preuve de pixels visibles.
`SPLASH_OK` reste absent. La reprise devra comparer le commit effectif du
compositeur Saturn, le routage scène DPU1, la cmdlist du canal 7, le plane
RDMA/DMMU et le chemin TMG.

## 18 octobre 2026 — horloges APMU et référence DP

Le relevé Linux du DPU1 confirme les fréquences utilisées par le pilote :
PXCLK environ `107142857 Hz` (mode DP réglé à `108000 kHz`), MCLK
`307200000 Hz` et ACLK `409600000 Hz`. Le firmware écrivait auparavant les
gates PXCLK/MCLK dans `LCD_CLK_RES_CTRL3`, qui est le registre de l'horloge
ESC. Le portage les applique maintenant dans `LCD_CLK_RES_CTRL4` et programme
explicitement les sélecteurs/diviseurs PXCLK, MCLK et ACLK selon l'arbre Linux.

Après reconstruction (`bonjour-jupiter2.bin`, `36477` octets), le flux normal
a confirmé `APMU_ENABLE_OK`, `LINK_TRAIN_OK`, `DP_MSA_STAGE_OK` et
`SCANOUT_STAGE_OK`. La mire interne DP a ensuite été reproduite avec :

```text
DP1+0x238_COLORBAR_RB=0x00000001
DP_COLORBAR_BLUE_HOLD
```

Le firmware du dépôt est revenu à `DP_COLORBAR_TEST=false` après ce contrôle ;
la prochaine exécution à transférer est donc le flux splash Saturn/RDMA.

## 18 octobre 2026 — watchdog hérité d'U-Boot identifié

La mire bleue disparaissait après environ une minute. L'UART a montré que
U-Boot démarre `watchdog@d4014000` avec un délai de 60 secondes et que ce
watchdog n'est plus rafraîchi après la commande `go`. Le firmware bare-metal
était donc réinitialisé pendant le test ; la disparition de la mire ne
provenait pas du lien USB-C/HDMI.

WDT0 est maintenant désactivé au tout début de `rust_main()` avec les clés
K3 documentées (`0xBABA`, `0xEB10`) et `TWER=0`. Le binaire de test a été
reconstruit à 35837 octets avec `DP_COLORBAR_TEST=true`, pour conserver la
mire DP bleue connue comme référence physique. La prochaine capture UART
doit contenir `WDT_DISABLED` et rester active au-delà de 60 secondes avant de
repasser au chemin Saturn/RDMA puis au splash.

## 18 octobre 2026 — mire bleue stable au-delà du timeout

L'image de 35837 octets a été chargée et lancée. L'UART a confirmé
`WDT0+0x0B8_RB=0x00000000`, `WDT_DISABLED`,
`DP1+0x238_COLORBAR_RB=0x00000001` et `DP_COLORBAR_BLUE_HOLD`. Après plus de
60 secondes, aucune nouvelle séquence U-Boot n'est apparue : le watchdog
hérité est neutralisé et la mire DP reste active. Le diagnostic matériel peut
maintenant reprendre avec Saturn/RDMA et le splash.

La mire bleue est visible sur le HP Compaq LA2006x et reste affichée. Le
transmetteur DP et toute la chaîne USB-C/HDMI sont validés ; le défaut restant
est limité au chemin de composition Saturn/RDMA vers le framebuffer.

## 18 octobre 2026 — handshake scène DPU1 corrigé

Le test Saturn pleine couleur validait les écritures `CMP1` sur l'UART, mais
ne produisait toujours aucune image. Le pilote constructeur montre que le
handshake de DPU1 doit utiliser le bloc scène à `0x340` : `both_cfg_rdy` à
`0x348` et `sw_start` à `0x350`. Le firmware les envoyait auparavant à
`0x388/0x390`, qui correspondent au bloc scène 2 et ne démarrent pas le flux
DPU1.

Le correctif a été compilé et copié dans
`tftp-root/bonjour-jupiter2.bin` (`40645` octets). La validation matérielle
reste à faire après reset et lancement U-Boot ; la trace attendue doit afficher
`DPU1+0x348_CFG_READY_RB` et `DPU1+0x350_SW_START_RB`.

## 18 octobre 2026 — flux actif, couleur Saturn à isoler

Le binaire corrigé a été chargé et lancé. L'UART confirme le handshake DPU1
sur `0x348/0x350`, `DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et les trois mises à
jour de couleur Saturn. Le moniteur affiche désormais un écran gris : le flux
vidéo est donc visible, mais la couleur de `layer00` n'est pas encore correcte.

Une image de diagnostic de `35981` octets est prête. Elle désactive `layer00`
et programme directement un fond rouge dans TMG0 pour isoler le compositeur
du chemin de sortie DP.

L'image a été exécutée après reset. L'UART confirme
`TMG0+0x24_BG_RG_RB=0x00000FFF`, `TMG0+0x28_BG_B_RB=0x00000000`,
`DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `TMG_BACKGROUND_RED_HOLD`. Le contrôle
visuel doit déterminer si ce fond rouge apparaît correctement.

Le rouge est visible sur le moniteur. Le chemin DP et le timing sont validés ;
le défaut est localisé à la configuration de `layer00` Saturn. Le prochain
binaire (`40685` octets) active `solid_area_mode` et remet le fond TMG au noir
pour retester la couche Saturn seule.

## 18 octobre 2026 — référence DP bleue confirmée après reset

La mire interne du transmetteur DP a été réactivée dans un binaire de référence
de `36029` octets. Après reset et lancement à `0x140000000`, l'UART a confirmé
`WDT_DISABLED`, `DP1+0x238_COLORBAR_RB=0x00000001`, `LINK_TRAIN_OK`,
`DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `DP_COLORBAR_BLUE_HOLD`. Le bleu est
visible sur le HP Compaq LA2006x. La liaison physique et le transmetteur sont
donc validés indépendamment de Saturn ; le prochain essai revient au chemin
`CMP1`/`layer00`.

## 18 octobre 2026 — correction du registre `solid_area_mode`

Le test Saturn `layer00` reste noir malgré les readbacks corrects. La lecture
du pilote constructeur `composer_x.h` a localisé une erreur de registre :
`solid_area_mode` est le bit 16 de `CMP1+0x34`, avec `solid_color_a`, tandis
que `CMP1+0x38` doit rester à `0x0000FF09` pour activer la couche et sa couleur
pleine. Le firmware écrivait auparavant le bit 16 dans `CMP1+0x38`, où il
correspond à `alpha_ratio`.

Le correctif ajoute `0x00010000` à la valeur d'alpha écrite à `CMP1+0x34` et
conserve `CMP1+0x38=0x0000FF09`. Le binaire est reconstruit dans
`tftp-root/bonjour-jupiter2.bin` (`40685` octets). Le prochain essai doit
confirmer `CMP1+0x34_SOLID_A_RB=0x000100FF`.

## 18 octobre 2026 — correction exécutée sur la Jupiter 2

Le binaire `bonjour-jupiter2.bin` (`40685` octets) a été reconstruit et chargé
par TFTP après reset. Le splash 1600×900 a été transféré à
`0x102000000` (`5760000` octets), puis l'image a été lancée à
`0x140000000`. La lecture UART confirme le nouveau placement de
`solid_area_mode` et l'activation de `layer00` :

```text
CMP1+0x34_SOLID_A_RB=0x000100FF
CMP1+0x38_ACTIVE_RB=0x0000FF09
DPU1+0x348_CFG_READY_RB=0x00000001
DPU1+0x350_SW_START_RB=0x00000000
RGB_CYCLE_SOLID_BLUE_OK
RGB_CYCLE_SOLID_RED_OK
RGB_CYCLE_SOLID_GREEN_OK
RGB_CYCLE_DONE
```

`DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `WDT_DISABLED` sont également
confirmés. L'exécution reste stable après le cycle de couleurs. Il reste à
associer cette trace à l'observation du moniteur avant de déclarer le premier
affichage Saturn visible.

## 18 octobre 2026 — vérification des champs Saturn HEE

La lecture directe du pilote constructeur public K3 (`dpu_saturn_hee.c` et les
en-têtes `composer_x.h`, `dpu_scene_ctl.h`, `reg_map_hee.h`, `cmdlist_top.h`)
a confirmé les champs de scène `timing_inter0` dans `DPU1+0x34C[6:1]` et
`timing_inter1` dans `[12:7]`. Elle a corrigé l'interprétation de la table :
`saturn_get_cl_cfg_rdy_addr()` place `CMDLIST_CFG_READY` à `DPU1+0x3F4`
(`DPU_CTRL_TOP+0x34`), tandis que `DPU1+0x534` est réservé dans la fenêtre
cmdlist. Les deux bits d'activation du composer 1 sont à `DPU1+0x3E8`
(`DPU_CTRL_TOP+0x28`), pas `0x3EC`. Ces corrections sont dans
`firmware/bonjour/src/main.rs`.
Le pilote `wb_cmdlist()` charge aussi la liste du compositeur sur le canal
`rdma_nums + 2 + crtc_id` ; avec quatre RDMA et DPU1/CRTC1, le canal correct
est 7, alors que l'image précédente programmait le canal 0.

La compilation locale est la prochaine validation ; la Jupiter 2 devra ensuite
être réinitialisée et recevoir le nouveau binaire avant de juger l'affichage.

L'exécution suivante a transféré le binaire corrigé par TFTP et atteint :

```text
CMDLIST_STAGE_OK
DPU1+0x3E8_RB=0x00000003
CMDLIST_ENABLE_OK
DPU1+0x34C_RB=0x00000411
DPU_SCENE_STAGE_OK
CMP1+0x34_SOLID_A_RB=0x000100FF
CMP1+0x38_ACTIVE_RB=0x0000FF09
DPU_LAYER_STAGE_OK
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SATURN_SOLID_BLUE_HOLD
```

Le firmware reste actif sur la couleur bleue solide Saturn ; la confirmation
visuelle du moniteur doit maintenant clôturer ce jalon.

## 18 octobre 2026 — essai instrumenté interrompu par HPD absent

Le firmware a été reconstruit avec des lectures UART supplémentaires après
`SW_START`, puis chargé par TFTP à `0x140000000` (`36461` octets). Cet essai a
confirmé les paramètres qui étaient suspects dans l'analyse précédente :

```text
RDMA0+0x3C_RB=0x00001900       # 6400 octets, 1600 x 4
TBU0+0x28_RB=0x0000057F        # 1407 pages
RDMA0+0x24_VA_RB=0x10000000
RDMA0+0x28_VA_RB=0x00000000
DMMU_STAGE_OK
CMDLIST_STAGE_OK
DPU_SCENE_STAGE_OK
DPU_LAYER_STAGE_OK
```

Le test n'a pas atteint `DPU_POST_COMMIT` : après le lancement, U-Boot et le
firmware n'ont pas détecté de HPD et les transactions AUX ont échoué :

```text
dp cannot get HPD signal
AUX_AFTER_DPU_FAIL
AUX_AFTER_PLL_FAIL
DPCD_REV=0x00000000
DPCD_RATE=0x00000000
DPCD_LANES=0x00000000
AUX_STAGE_READBACK_FAIL
```

Cette trace ne permet donc pas de conclure sur le compositeur. Le prochain
essai doit commencer avec l'écran physiquement relié, la bonne entrée
sélectionnée et le HPD présent ; les lectures `DPU_POST_COMMIT` pourront alors
être observées après `SW_START`.

## 18 octobre 2026 — vrai test Saturn bleu après reset

Le drapeau de diagnostic `CMP_BACKGROUND_TEST` a été remis à `false`. L'essai
précédent ne testait donc pas `layer00` : il désactivait volontairement la
couche (`CMP1+0x38=0`) pour isoler le fond CMP. Après un reset réellement
observé sur l'UART, le binaire de `36405` octets a été chargé par TFTP à
`0x140000000`, vérifié par `md.b`, puis exécuté.

La trace confirme cette fois la couche pleine Saturn et le maintien du flux :

```text
CMP1+0x38_ACTIVE_RB=0x0000FF09
DPU1+0x348_CFG_READY_RB=0x00000001
TMG0+0x00_ACTIVE_RB=0x00000181
DPU_POST_COMMIT_END
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SATURN_SOLID_BLUE_HOLD
```

Le commit post-scène conserve `CMP1+0x38=0x0000FF09`, `CMP1+0x34=0x000100FF`
et `CMP1+0x30=0x000003FF`; le watchdog est désactivé et le lien DP est
entraîné (`LINK_TRAIN_OK`). La confirmation visuelle de l'écran reste à
associer à cette exécution matérielle.

## 18 octobre 2026 — test Saturn direct instrumenté et relancé

Le firmware a été recompilé après ajout de marqueurs explicites dans l'UART,
puis transféré par TFTP à `0x140000000` (`36501` octets). Après le reset, la
trace a confirmé le mode exact du test :

```text
SATURN_SOLID_TEST_BEGIN
SATURN_MODE=SOLID_DIRECT
SATURN_RDMA_SOURCE=DISABLED
SATURN_SOLID_RGB10=0,0,1023
SATURN_SOLID_A_CFG=0x00000000
CMP1+0x28_SOLID_R_RB=0x00000000
CMP1+0x2C_SOLID_G_RB=0x00000000
CMP1+0x30_SOLID_B_RB=0x000003FF
CMP1+0x34_SOLID_A_RB=0x000000FF
CMP1+0x38_ACTIVE_RB=0x0000FF09
DPU_LAYER_STAGE_OK
```

La cmdlist compositeur et le chemin RDMA ont bien été isolés :
`DPU1+0x3E8_RB=0x00000000`, `RDMA0+0xB4_POST_RB=0x00000000` et aucune IRQ
d'erreur DPU/RDMA n'est signalée. Le lien physique et le commit de scène
aboutissent également :

```text
LINK_TRAIN_OK
DPU_POST_COMMIT_END
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SATURN_SOLID_BLUE_HOLD
```

`AUX_AFTER_PLL_FAIL` apparaît encore comme diagnostic intermédiaire, mais la
séquence AUX de récupération atteint ensuite `AUX_STAGE_OK` et
`LINK_TRAIN_OK`. Cette exécution prouve le chemin de contrôle jusqu'au maintien
du bleu solide ; la couleur effectivement observée sur le moniteur doit encore
être associée à cette trace. Le splash BGRA n'est pas activé dans ce test.
