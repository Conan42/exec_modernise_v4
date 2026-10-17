# Bring-up Milk-V Jupiter 2

## État confirmé — 16 octobre 2026

Le premier bring-up réel est passé jusqu'au prompt U-Boot. La carte est une
**SpacemiT K3 Pico ITX** équipée de 16 GiB de LPDDR5. Le boot démarre depuis
la NOR, détecte ensuite l'UFS, puis charge normalement Bianbu si l'autoboot
n'est pas arrêté.

La carte a été arrêtée au prompt U-Boot `=>` par une modification temporaire
de `/boot/env_k3.txt`. Le dernier essai l'a laissée dans la boucle `WFI` du
firmware de diagnostic après le jalon APMU ; un redémarrage est nécessaire pour
retrouver le prompt U-Boot.

## Liaison actuelle — niveau logique 1,8 V

> **Ne pas utiliser 3,3 V. La liaison UART DEBUG de la Jupiter 2 est en
> logique 1,8 V.** Cette précision concerne le câblage électrique, pas
> seulement la configuration du terminal.

Le script [`scripts/monitor-uart.sh`](scripts/monitor-uart.sh) utilise
`/dev/ttyUSB0` à `115200` bauds, `8N1`, sans contrôle de flux. V4 reprend
exactement ces paramètres dans `jupiter2ctl monitor`.

La liaison USB/UART DEBUG utilise donc un **niveau logique de 1,8 V** : GND est
commun, TXD de l'adaptateur va vers RXD de la carte et RXD de l'adaptateur va
vers TXD de la carte. Utiliser un adaptateur USB-UART compatible 1,8 V, ou un
convertisseur de niveau adapté. La documentation Jupiter 1 en 3,3 V ne doit
pas être réutilisée pour ce câblage.

Le convertisseur détecté par l'hôte est un **FTDI FT232R** sur
`/dev/ttyUSB0`. Le compte de travail doit appartenir au groupe Linux
`dialout`.
Une session de terminal a permis de capturer le démarrage et de laisser le
prompt U-Boot ouvert.

## Séquence de boot capturée

Les premières lignes utiles sont :

```text
U-Boot SPL 2022.10
U-Boot 2022.10
Model: spacemit k3 pico itx board
LPDDR5 total size: 16 GB
DRAM: 16 GiB
```

La NOR détectée est une `w25q64dw` de 8 MiB. U-Boot charge l'environnement
`env_k3.txt` depuis la partition boot UFS, puis Linux Bianbu 4.0.7 démarre avec
Linux `6.18.3-generic` lorsque l'autoboot est laissé actif. La partition boot
est `/dev/sda2`, en ext4 et montée en lecture-écriture. Elle contient :

```text
/boot/env_k3.txt
/boot/vmlinuz-6.18.3-generic
/boot/initrd.img-6.18.3-generic
/boot/spacemit/6.18.3-generic/k3-pico-itx.dtb
```

Le système Linux a aussi confirmé 16 CPU, le RGX `BVNC 36.56.104.183` et
l'initialisation DPU/DisplayPort en `1920x1080`.

## Arrêt temporaire du boot

La touche d'interruption standard n'a pas été identifiée pendant le compte à
rebours. La méthode retenue consiste à modifier l'environnement lu depuis
`/boot` une fois Linux démarré, avec privilèges administrateur :

```bash
sudo cp -a /boot/env_k3.txt /boot/env_k3.txt.boot_bak
sudo sh -c 'printf "bootdelay=5\\nbootcmd=echo V4_BOOT_HALTED\\n" > /boot/env_k3.txt'
```

Avant le remplacement, une copie intermédiaire avait été conservée après
l'ajout du délai : `/boot/env_k3.txt.bootdelay_bak`. L'environnement original
contenait notamment `bootdelay=0`, `bootcmd=run autoboot` et une chaîne
`ufs_boot` qui chargeait le noyau, le DTB et l'initrd depuis UFS.

Après redémarrage, le résultat attendu et obtenu est :

```text
load env_k3.txt from bootfs successful
Autoboot in 5 seconds
V4_BOOT_HALTED
=>
```

Pour revenir au boot Linux, la restauration doit être faite depuis un Linux
redémarré ou un autre accès au volume :

```bash
sudo cp -a /boot/env_k3.txt.boot_bak /boot/env_k3.txt
sync
```

La restauration n'a pas été exécutée : le prompt U-Boot est conservé pour la
prochaine étape réseau.

## U-Boot et réseau

La version relevée au prompt est `U-Boot 2022.10 (Sep 07 2026 - 11:31:13
+0000)`, build `jenkins-BSP-build-bsp-deb-513`. Les paramètres observés
comprennent :

```text
serverip=10.0.92.148
ipaddr        (non défini)
ethaddr=50:0A:52:0D:19:EC
stdin=serial,cros-ec-keyb,usbkbd,usbkbd1
stdout=serial
stderr=serial
```

L'hôte utilise actuellement l'interface `enp3s0` avec
`192.168.1.131/24`. Lors du premier relevé, aucun voisin correspondant à la
Jupiter 2 n'avait été détecté sur ce réseau. `serverip=10.0.92.148` provenait
de l'environnement U-Boot et ne constituait pas une preuve que l'hôte était
joignable sur ce sous-réseau.

Le câble Ethernet est maintenant confirmé : U-Boot négocie `eth0` et le ping
vers le PC `192.168.1.131` réussit avec la configuration volatile suivante :

```text
ipaddr=192.168.1.132
serverip=192.168.1.131
netmask=255.255.255.0
```

Ces valeurs n'ont pas été enregistrées dans l'environnement NOR/UFS ; elles
restent actives uniquement dans la session U-Boot courante.

La prochaine étape était de lancer le serveur TFTP sur UDP 69, puis d'envoyer
une RRQ depuis U-Boot. Cette étape est maintenant réussie.

## TFTP

Le bootloader est le client TFTP : il envoie une requête `RRQ` au PC. V4
fournit le serveur dans `host/jupiter2ctl`. La racine par défaut est
`tftp-root/`, qui doit être créée et recevoir explicitement l'image à servir.

```bash
mkdir -p tftp-root
cargo run -p jupiter2ctl -- tftp-serve --bind 0.0.0.0:69 --root tftp-root
```

Le serveur refuse les chemins absolus et les traversals `..`. Il ne sert que le
mode TFTP `octet`, avec des blocs de 512 octets et trois tentatives par bloc.

Le serveur TFTP local et son smoke test UDP sont validés. Le transfert réel
avec U-Boot est également validé ci-dessous. Ne pas utiliser `10.0.92.148`
comme cible réseau : cette ancienne valeur provenait de l'environnement U-Boot
et ne correspondait pas à l'adresse actuelle du PC.

Sur le PC Ryzen 5, l'interface active est `enp3s0` (`192.168.1.131/24`). Un
fichier de test `tftp-root/jupiter2-test.bin` a été servi avec succès par une
requête RRQ locale. Le serveur tourne actuellement sur UDP `1069`, car le
compte courant ne peut pas ouvrir le port privilégié 69 :

```bash
cargo run -p jupiter2ctl -- tftp-serve \
  --bind 0.0.0.0:1069 --root tftp-root
```

Pour le test avec U-Boot, le serveur devra être lancé sur UDP 69 avec les
droits administrateur :

```bash
cargo build -p jupiter2ctl
sudo /path/to/exec_modernise_v4/target/debug/jupiter2ctl \
  tftp-serve --bind 0.0.0.0:69 \
  --root /path/to/exec_modernise_v4/tftp-root
```

Le serveur sur 1069 validait le logiciel mais ne suffisait pas au bootloader,
qui utilise le port TFTP standard 69. Le serveur est maintenant lancé sur 69
avec les droits administrateur du PC.

Le premier transfert réel a été effectué avec :

```text
TFTP from server 192.168.1.131; our IP address is 192.168.1.132
Filename 'jupiter2-test.bin'.
Load address: 0x102000000
Bytes transferred = 22 (16 hex)
```

Le serveur a confirmé la RRQ depuis `192.168.1.132:2079`. Cette étape valide
le réseau, le client TFTP U-Boot et le serveur V4. Le fichier de test n'est
pas un firmware et n'a pas été exécuté.

## Vérifications V4 déjà passées

Les commandes suivantes ont été validées dans le workspace :

```text
cargo run -p jupiter2ctl -- info
cargo test --workspace        # 142 tests
```

Le moniteur UART V4 ouvre le FTDI et le serveur TFTP répond aux tests locaux.
Ces validations concernent l'outillage hôte ; elles ne constituent pas encore
une exécution du firmware V4 sur la Jupiter 2.

## Prochain jalon — splash BGRA et journal UART

Le prochain essai vise un affichage statique sans shaders. L'asset préparé est
`assets/images/splashscreen-1920x1080.bgra` :

```text
largeur       1920 pixels
hauteur       1080 pixels
format        4 octets par pixel, BGRA8 brut
stride        7680 octets
taille        8294400 octets
```

Une copie destinée au prochain transfert est placée dans
`tftp-root/splashscreen-1920x1080.bgra`. Le chargement TFTP peut utiliser
temporairement l'adresse déjà validée `0x102000000` :

```text
tftpboot 0x102000000 splashscreen-1920x1080.bgra
```

Cette commande ne lance pas l'image. Elle ne fait que déposer le framebuffer
brut en DRAM. Le firmware V4 devra ensuite publier des étapes UART distinctes,
par exemple `UART_OK`, `DRAM_OK`, `FB_OK`, `DPU_OK`, `DP_OK` et `SPLASH_OK`.

L'affichage direct ne demande pas encore le GPU RGX ni les shaders SPIR-V. Le
bloc à valider est le chemin **DPU/scanout/DisplayPort** : horloges, resets,
power domains, format de couche, adresse DMA ou DMMU, timings et activation du
connecteur. Le manuel K3 décrit les capacités et les adresses des blocs, mais
la séquence complète de registres doit être reprise du device tree et du
pilote Linux Bianbu avant toute écriture bare-metal.

Le format BGRA du fichier est donc fixé comme donnée d'entrée, tandis que la
correspondance exacte avec `ARGB8888`/permutation RB du plane DPU sera vérifiée
sur la carte. Une image aux couleurs inversées sera corrigée par le réglage du
plane ou par une conversion préparée, sans introduire de shader.

### Image firmware Bonjour

La première image RISC-V bare-metal est `firmware/bonjour`. Elle est liée à
`0x140000000`, séparée du framebuffer splash à `0x102000000`, et écrit les
jalons `Bonjour Jupiter 2 V4`, `UART_OK` puis un contrôle de lecture du splash
sur la console DEBUG. Le binaire plat se construit avec :

```bash
./scripts/build-bonjour.sh
```

Le transfert et le lancement, après démarrage du serveur TFTP, seront :

```text
=> tftpboot 0x102000000 splashscreen-1920x1080.bgra
=> tftpboot 0x140000000 bonjour-jupiter2.bin
=> go 0x140000000
```

Le premier essai ne programme pas encore le DPU. Son résultat attendu est un
log UART `Bonjour Jupiter 2 V4`, `UART_OK` et `SPLASH_RAM_READ`. L'absence de
sortie permettra d'abord de vérifier le choix de l'adresse UART, son horloge,
son reset et l'hypothèse d'état laissée par U-Boot avant d'aborder le display.

### Preuve obtenue — 17 octobre 2026

Le test réel est réussi avec le PC `192.168.1.131` et la Jupiter
`192.168.1.132`. U-Boot a transféré les deux fichiers, puis a lancé l'image
plate à `0x140000000` :

```text
TFTP splashscreen-1920x1080.bgra : 8294400 octets
TFTP bonjour-jupiter2.bin       : 5864 octets
go 0x140000000
## StartingBonjour Jupiter 2 V4
UART_OK
DRAM_OK
SPLASH_RAM_READ
MMIO_PROBE_BEGIN
DPU1+0x000=0x08040030
DPU1+0x340=0x00000000
DPU1+0x380=0x00000000
DPU1+0x3C0=0x00000000
DP1+0x000=0x00000000
DP1+0x00C=0x02A1350D
MMIO_PROBE_OK
```

Le serveur TFTP a confirmé les deux RRQ depuis la Jupiter. Cette preuve valide
le transfert du splash, le transfert et l'exécution du firmware RISC-V, l'accès
UART bare-metal et la lecture de la zone DRAM du splash. Le DPU/DisplayPort
n'est pas encore initialisé : l'image a été chargée en mémoire mais n'est pas
encore affichée par le scanout matériel.

### Preuve MMIO DPU1/DP1 — 17 octobre 2026

Le device tree du Pico-ITX active la sortie `dp1`/eDP1, et non `dp0` :

```text
DPU1 : 0xC044_0000, fenêtre Saturn 0x54000
DP1  : 0xCAC8_8000, fenêtre 0x4000
APMU : 0xD428_2800
```

U-Boot avait déjà identifié le sink sur cette sortie (`DPCD Rev 1.2`,
`MaxRate 5400000`, `MaxLanes 2`) mais avait terminé par `Failed to enable
display`. Depuis le firmware V4, les lectures U-Boot et bare-metal concordent :

```text
DPU1+0x000=0x08040030
DPU1+0x340=0x00000000
DPU1+0x380=0x00000000
DPU1+0x3C0=0x00000000
DP1+0x000=0x00000000
DP1+0x00C=0x02A1350D
```

Le firmware n'écrit encore aucun registre graphique. `DPU1+0x000` fournit la
signature/version Saturn et `DP1+0x00C` est conservé comme valeur brute ; les
autres offsets sont publiés pour permettre la comparaison avec les captures
Linux. Le jalon `MMIO_PROBE_OK` prouve que le code RISC-V peut lire les fenêtres
DPU1 et DP1 après le transfert TFTP.

### Préparation APMU — première écriture contrôlée

La sonde APMU précédente a validé les quatre registres qui portent les
horloges et resets du chemin DPU1/eDP1 :

```text
APMU+0x23C = APMU_LCD_EDP_CTRL
APMU+0x26C = APMU_LCD_CLK_RES_CTRL3
APMU+0x270 = APMU_LCD_CLK_RES_CTRL4
APMU+0x274 = APMU_LCD_CLK_RES_CTRL5
```

Le firmware publie `APMU_PROBE_BEGIN`, les quatre mots 32 bits et
`APMU_PROBE_OK`. Ces valeurs ont été comparées à une lecture U-Boot au même
boot :

```text
U-Boot  0xD4282A3C = 0x00070000
        0xD4282A6C = 0x0080401C
        0xD4282A70 = 0x01A9C343
        0xD4282A74 = 0x00000003

V4      APMU+0x23C = 0x00070000
        APMU+0x26C = 0x0080401C
        APMU+0x270 = 0x01A9C343
        APMU+0x274 = 0x00000003
```

La nouvelle image ajoute `APMU_ENABLE_BEGIN`. Elle effectue une
lecture-modification-écriture limitée aux gates d'horloge et aux bits de
désassertion des resets DPU1/eDP1, puis vérifie chaque lecture de retour. Le
succès sera publié par `APMU_ENABLE_OK`. Cette image ne touche pas encore aux
registres Saturn, DMMU, RDMA, cmdlist ou DP.

### Preuve APMU active — 17 octobre 2026

Après redémarrage automatique par le watchdog, le splash et l'image de
diagnostic ont été transférés par TFTP puis exécutés. La séquence UART a
confirmé :

```text
APMU_ENABLE_BEGIN
APMU+0x26C_RB=0x0081401D
APMU+0x270_RB=0x01A9C343
APMU+0x274_RB=0x00000003
APMU+0x23C_RB=0x00070000
APMU_ENABLE_OK
```

`CTRL3` est passé de `0x0080401C` à `0x0081401D` : les bits de gate MCLK et
PXCLK ont été activés, tandis que les autres registres ont conservé leurs
valeurs. La lecture de retour confirme l'écriture. Le firmware reste ensuite
dans sa boucle `WFI`; aucun registre Saturn, DMMU, RDMA, cmdlist ou DP n'est
encore programmé.

La nouvelle image de diagnostic a été transférée à `0x140000000` et exécutée
après le chargement du splash. Elle a produit `APMU_PROBE_OK`, puis
`APMU_ENABLE_OK`, avant de rester dans sa boucle `WFI`.

### Initialisation Saturn minimale — 17 octobre 2026

Après correction du mapping `CMP1_BASE_ADDR` (`DPU1 + 0x30000` pour le
pipeline 1), l'image a validé les registres de ressources Saturn :

```text
DPU1+0x1284_RB=0x00000810
DPU1+0x1288_RB=0xFFFFFFFE
DPU1+0x1200_RB=0xFFFF0000
DPU1+0x1208_RB=0x00000008
DPU1+0x120C_RB=0x00000008
DPU1+0x30000_RB=0x00000001
DPU_REG_INIT_OK
```

Le composer est activé sans couche et aucun démarrage de scanout n'est
effectué. Le prochain portage concerne le timing 1920×1080, le plane RDMA et
la table DMMU du framebuffer BGRA.

### Timing Saturn préparé — 17 octobre 2026

Le pipeline DP1 utilise `CMP1` pour le composer et `TMG0` (`DPU1 + 0x51200`)
pour le timing. Le firmware a programmé le mode 1920×1080@60 suivant, avec
`fm_timing_en` laissé à zéro :

```text
pixel clock cible : 148500 kHz
htotal            : 2200 (1920 + 88 + 44 + 148)
vtotal            : 1125 (1080 + 4 + 5 + 36)
format sortie     : RGB888
```

La capture UART réelle confirme les lectures de retour :

```text
TIMING_STAGE_BEGIN
TMG0+0x00_RB=0x00000180
TMG0+0x08_RB=0x00002000
TMG0+0x0C_RB=0x00940058
TMG0+0x10_RB=0x0094002C
TMG0+0x14_RB=0x00050004
TMG0+0x18_RB=0x07800024
TMG0+0x1C_RB=0x00000438
TMG0+0x3C_RB=0x00000001
TIMING_STAGE_OK
```

Cette étape valide les offsets et les valeurs du bloc de timing, sans lancer
de scanout. Le plane RDMA, le DMMU, la cmdlist, le PLL pixel et le link
training DP restent à configurer avant d'activer le flux vidéo.

### Plane RDMA préparé — 17 octobre 2026

Le firmware a ensuite préparé `RDMA0` et la couche 0 de `CMP1`, toujours sans
activer de lecture mémoire ni de scanout :

```text
RDMA0+0x00_RB=0x00002000
RDMA0+0x24_RB=0x02000000
RDMA0+0x28_RB=0x00000001
RDMA0+0x3C_RB=0x00001E00
RDMA0+0x40_RB=0x04380780
RDMA0+0x44_RB=0x00000000
RDMA0+0x48_RB=0x0437077F
RDMA0+0x74_RB=0x00000007
CMP1+0x04_RB=0x04380780
CMP1+0x14_RB=0x000000FF
CMP1+0x20_RB=0x077F0000
CMP1+0x24_RB=0x04370000
CMP1+0x38_RB=0x00000000
RDMA_STAGE_OK
```

L'adresse haute et basse reconstituent `0x102000000`, le stride vaut `7680`
octets, et le format Saturn `7` correspond à `BGRA8888` dans le pilote Linux.
Le DMMU et la cmdlist sont encore absents ; le bit d'activation de la couche
reste donc à zéro.

Le pilote Linux K3 apporte maintenant la séquence nécessaire au portage :
power-domain LCD1, désassertion des cinq resets DPU1, activation des horloges
`DSI4LN2_LCD_PXCLK`, `DSI4LN2_LCD_MCLK`, `DSI4LN2_DPU_ACLK`, `DSI4LN2_DSI_ESC`
et `DSI4LN2_LCD_DSC`, puis reset/horloge de `EDP1_PXCLK` et reset DP1. Il
initialise ensuite le DPU Saturn, la table DMMU du plane et la cmdlist RDMA,
avant le link training AUX et l'activation du flux vidéo DP. Cette chaîne doit
être portée dans cet ordre ; un simple `framebuffer address + enable` ne suffit
pas.

## État matériel

Les adresses documentées du K3 ne sont pas encore transformées en driver V4 :

- UART0 : `0xD401_7000` ;
- RGX : `0xCAC0_0000` ;
- V2D : `0xC010_0000` ;
- DPU0 : `0xC034_0000` ;
- DPU1 : `0xC044_0000` ;
- IOMMU : `0xC0F0_0000`.

Ces adresses seules ne suffisent pas à initialiser le matériel. Les clocks,
resets, power-domains, IOMMU, DMA et IRQ doivent être portés depuis le device
tree et validés sur la carte.

## Validation AUX, link training et premier scanout — 17 octobre 2026

Le switch clavier/souris/écran a été placé sur l'entrée Jupiter 2. U-Boot a
alors confirmé un HPD actif dans `DP1+0x088` et a lu le sink : DPCD 1.2,
débit maximal 5,4 Gb/s et deux voies. Une lecture AUX brute U-Boot a également
retourné l'événement `0x00010000` et les données DPCD `0x81C21412`.

Le premier helper bare-metal utilisait des lectures-modifications-écritures
pour `AUX_CONFIG` et le strobe `AUX_START`. Le registre START est une commande,
pas un registre d'état ; il doit recevoir directement `0` puis `1 << 31`.
Après correction, la séquence AUX dédiée a produit :

```text
DPCD_REV=0x00000012
DPCD_RATE=0x00000014
DPCD_LANES=0x000000C2
AUX_INT_RB=0x00010000
AUX_STATUS_RB=0x0004B149
AUX_CONFIG_RB=0x0F000009
AUX_START_RB=0x00000000
AUX_DATA1_RB=0x81C21412
AUX_STAGE_OK
```

DP1 est le DisplayPort externe (`dp-id = 1` dans `k3-dp1.dtsi`). Le bit
`ENABLE_EDP` reste donc à zéro ; le forcer provoquait un mauvais mode de
contrôleur. L'AUX est réinitialisé avant la lecture DPCD, puis le firmware
programme HBR 2,70 Gb/s, deux voies et l'encodage amélioré.

Le link training réel a ensuite réussi :

```text
LINK_CR_STATUS=0x00000000
LINK_CR_STATUS=0x00000000
LINK_CR_STATUS=0x00000011
LINK_EQ_STATUS=0x00000077
LINK_TRAIN_OK
```

Le firmware a programmé le MSA 1920×1080 RGB888, le flux DP et le timing Saturn
(`TMG0+0x00_ACTIVE_RB=0x00000181`). La capture finale contient
`DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `MMIO_PROBE_OK`. Le marqueur
`SPLASH_OK` reste réservé à la confirmation visuelle de l'image sur le
moniteur ; le switch doit rester sur Jupiter 2 pendant cette vérification.

## Résultat écran et prochain essai

Le premier essai avec l'écran actuellement relié au switch n'a produit aucun
splash visible. La capture UART confirme pourtant la séquence complète
`AUX_STAGE_OK`, `LINK_TRAIN_OK`, `DP_MSA_STAGE_OK` et `SCANOUT_STAGE_OK`.
`SPLASH_OK` ne doit donc pas encore être déclaré : l'image n'est pas validée
visuellement.

Le prochain essai se fera avec le HP Compaq LA2006x, après bascule du switch
sur la sortie de la Jupiter 2. Cet écran est probablement natif en 1600×900,
alors que le firmware programme actuellement un mode fixe 1920×1080. Il faudra
lire son EDID ou ajouter un mode 1600×900 avant de conclure sur le scanout.

## Diagnostic sans signal — correctif préparé le 17 octobre 2026

Le premier essai avec le HP Compaq LA2006x directement relié à la Jupiter 2
reste sans signal. Le link training DP et les lectures AUX précédentes ne
suffisent donc pas à déclarer le scanout fonctionnel.

Le firmware `bonjour` a été corrigé pour le prochain essai :

- `PHY_BUSY_BYP` reste actif après le link training, comme dans le pilote Linux ;
- la table DMMU arrondit la taille du framebuffer à 1407 pages de 4 KiB et
  conserve les 60 entrées de garde ;
- l'en-tête cmdlist attend les événements reload RDMA et composer, bits 4 et 6,
  avant la ligne `CFG_RDY`.

Le binaire reconstruit mesure 30 781 octets. Il n'a pas encore été exécuté sur
la carte : la console UART reste silencieuse tant qu'un reset physique de la
Jupiter 2 n'est pas effectué.

## Exécution du correctif cmdlist — 17 octobre 2026

Après reset physique, U-Boot a de nouveau détecté le HP Compaq LA2006x en
1600×900, DPCD 1.2, HBR2 et deux voies. Le transfert réel a réussi :

```text
splashscreen-1600x900.bgra : 5760000 octets à 0x102000000
bonjour-jupiter2.bin        : 30781 octets à 0x140000000
```

La nouvelle exécution a confirmé les corrections :

```text
DMMU_TABLE_FIRST=0x00102000
DMMU_TABLE_LAST=0x0010257E
TBU0+0x28_RB=0x0000057F
DMMU_STAGE_OK
CMDLIST_HEADER=0x81000E0000000002
CMDLIST_STAGE_OK
CMDLIST_ENABLE_OK
DP1+0x100_POST_TRAIN_RB=0x80060121
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
MMIO_PROBE_OK
```

`AUX_AFTER_PLL_FAIL` reste un diagnostic transitoire ; la remise à zéro AUX
suivante rétablit `AUX_STAGE_OK` et le link training. La validation visuelle du
splash reste nécessaire avant d'ajouter `SPLASH_OK`.
## 17 octobre 2026 — timing confirmé par l'utilisateur

Le HP Compaq LA2006x est confirmé en **1600×900 à 60 Hz**. La résolution et
la fréquence ne suffisent pas à fixer les porches : le firmware essayait la
variante CVT reduced blanking à 97,5 MHz. Le binaire suivant essaie le timing
DMT standard du mode 1600×900@60 : HFP/HSYNC/HBP `24/80/96`, VFP/VSYNC/VBP
`1/3/96`, pixel clock `108000 kHz`, polarités positives. Le PLL pixel a été
mis en cohérence avec l'entrée Linux 108 MHz (`fbdiv=117`, `divaux=13`).

La reconstruction locale est passée (`cargo fmt --all -- --check`, build
firmware et `git diff --check`) ; `tftp-root/bonjour-jupiter2.bin` mesure
30 781 octets. Ce binaire doit encore être chargé et exécuté après un reset
physique de la Jupiter 2 avant toute conclusion visuelle.

## 17 octobre 2026 — exécution du timing DMT 1600×900@60

Après reset, le script UART/TFTP a chargé le framebuffer `splashscreen-1600x900.bgra`
(5 760 000 octets à `0x102000000`) et `bonjour-jupiter2.bin` (30 781 octets à
`0x140000000`). La Jupiter a confirmé le mode DPCD 1.2, HBR2 et deux voies.

Le firmware a appliqué le timing DMT 1600×900 à 60 Hz et son PLL pixel 108 MHz :

```text
TMG0+0x0C_RB=0x00600018
TMG0+0x10_RB=0x00600050
TMG0+0x14_RB=0x00030001
TMG0+0x18_RB=0x06400060
TMG0+0x1C_RB=0x00000384
DP1+0x190_RB=0x753001B2
```

La trace complète a confirmé `DMMU_STAGE_OK`, `CMDLIST_ENABLE_OK`,
`LINK_TRAIN_OK`, `DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `MMIO_PROBE_OK`.
`SPLASH_OK` reste réservé à la confirmation visuelle sur le HP Compaq LA2006x.

## 17 octobre 2026 — chaîne physique USB-C vers HDMI

La sortie observée n'est pas un connecteur DisplayPort directement relié au
moniteur : la Jupiter 2 utilise un adaptateur **USB-C/HDMI**. Le bloc `DP1`
reste le transmetteur source côté Jupiter ; l'adaptateur convertit le
DisplayPort Alt Mode transporté par l'USB-C en HDMI. Les lectures DPCD et le
link training valident donc Jupiter → adaptateur, mais pas encore la
synchronisation HDMI finale avec le HP Compaq.

## 17 octobre 2026 — HBR2 et statut après activation vidéo

Le sink annonce HBR2 (`DPCD_RATE=0x14`) sur deux voies. Le firmware a donc été
passé de HBR à HBR2, avec la PLL cœur Linux 5,4 Gb/s et les calculs MSA/TU
correspondants. Le transfert et l'exécution du nouveau binaire ont réussi.

La trace UART contient :

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

`LINK_POST_STATUS=0x77` montre que les six indicateurs de liaison restent
valides après l'activation du flux. L'écran HP Compaq LA2006x reste pourtant
sans image. Le chemin Jupiter → DP Alt Mode → adaptateur USB‑C/HDMI est donc
à isoler d'un problème de timing MSA ou de compatibilité du convertisseur ;
`SPLASH_OK` n'est pas déclaré.

## 17 octobre 2026 — référence Bianbu et passage V4 en RBR

Le boot Bianbu a été relancé avec le même adaptateur USB‑C/HDMI. Son pilote
Linux confirme la chaîne fonctionnelle et choisit le mode suivant :

```text
DP: Attempting Config: R=1620000, L=2
Setting PLL to Rate 1620000 kHz, Pclk 108000 kHz
DP: Training successful for R:1620000 L:2.
MSA: 1600x900, Rate:1620000 kHz, Lanes:2, BPP:24, TU:64.0
```

Le firmware V4 a été aligné sur cette configuration RBR : PLL cœur 1,62 Gb/s,
débit AUX `0x06`, deux voies, intervalle MSA et TU recalculés. L'exécution
bare-metal confirme :

```text
DP1+0x180_RB=0x873002B0
DP1+0x100_POST_TRAIN_RB=0x80060120
LINK_TRAIN_OK
DP1+0x200_VIDEO_RB=0x10400000
LINK_POST_STATUS=0x00000077
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
```

La chaîne DP est maintenant configurée comme Bianbu. `SPLASH_OK` attend encore
la confirmation visuelle sur le HP Compaq LA2006x.

## 17 octobre 2026 — confirmation du point d'arrêt V4

Une capture UART réalisée après reset a confirmé que U-Boot atteint le point
d'arrêt configuré dans `/boot/env_k3.txt` :

```text
[   9.462] V4_BOOT_HALTED
=>
```

À ce stade, U-Boot n'a pas chargé ni exécuté `bonjour-jupiter2.bin` ; aucune
conclusion supplémentaire ne peut donc être tirée sur le scanout ou le
splash. La commande `poweroff` a été testée depuis ce prompt, mais la carte
restait joignable sur le réseau. Pour une vraie mise hors tension, couper
l'alimentation de la Jupiter 2 avec son interrupteur ou son câble.

## 17 octobre 2026 — exécution V4 après reset

Après le reset, U-Boot ne possédait plus `ipaddr`. Les variables réseau ont
été définies manuellement : `ipaddr=192.168.1.132`, `serverip=192.168.1.131`
et `netmask=255.255.255.0`. Les transferts TFTP ont alors réussi :

```text
splashscreen-1600x900.bgra : 5760000 octets à 0x102000000
bonjour-jupiter2.bin        : 30861 octets à 0x140000000
```

La commande `go 0x140000000` a exécuté le firmware jusqu'aux marqueurs
`TIMING_STAGE_OK`, `RDMA_STAGE_OK`, `DMMU_STAGE_OK`, `CMDLIST_ENABLE_OK`,
`LINK_TRAIN_OK`, `DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et `MMIO_PROBE_OK`.
Le chemin bare-metal est donc exécuté avec succès ; `SPLASH_OK` attend encore
la confirmation visuelle sur le HP Compaq LA2006x.
