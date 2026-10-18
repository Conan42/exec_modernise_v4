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

## 17 octobre 2026 — correction du minutage Saturn après « pas de signal »

La dernière exécution atteint encore `LINK_TRAIN_OK`, `DP_MSA_STAGE_OK` et
`SCANOUT_STAGE_OK`, mais le HP Compaq LA2006x reste sans signal. La comparaison
avec le pilote SpacemiT Linux a révélé une erreur de packing dans `TMG0+0x0C`.
Ce registre contient `split_overlap[9:0]` et `HFP[27:16]`; `HBP` est dans
`TMG0+0x10`.

Le firmware écrivait auparavant `HFP | (HBP << 16)`, soit `0x00600018` pour
1600×900. Cela plaçait `24` dans `split_overlap` et `96` dans le champ `HFP`,
alors que `TMG0+0x10` portait déjà `HBP=96`. Le timing produit par Saturn ne
correspondait donc pas au timing MSA annoncé au transmetteur DP.

Le correctif écrit maintenant uniquement `HFP << 16` :

```text
TMG0+0x0C_RB=0x00180000
TMG0+0x10_RB=0x00600050
```

Le reset vidéo DP est aussi aligné sur le pilote Linux : les quatre bits du
champ `VIDEO_RESET[3:0]` sont assertés puis relâchés ensemble. Le binaire
corrigé a été compilé et copié dans `tftp-root/bonjour-jupiter2.bin`; il n'est
pas encore validé sur la Jupiter 2.

Après reset et arrêt à `V4_BOOT_HALTED`, le prochain essai est :

```text
setenv ipaddr 192.168.1.132
setenv serverip 192.168.1.131
setenv netmask 255.255.255.0
ping 192.168.1.131
tftpboot 0x102000000 splashscreen-1600x900.bgra
tftpboot 0x140000000 bonjour-jupiter2.bin
go 0x140000000
```

La validation attendue est d'abord la nouvelle valeur `TMG0+0x0C_RB`, puis la
confirmation visuelle du splash. `SPLASH_OK` reste réservé à cette observation
sur le moniteur.

Le premier lancement automatique de cette séquence n'a pas atteint le
firmware : après le chargement réseau, U-Boot a affiché plusieurs `T` puis
`Retry count exceeded` sur le transfert de `bonjour-jupiter2.bin`. Le service
TFTP présent sur UDP 69 ne répondait plus aux RRQ de la carte, alors qu'une
RRQ locale fonctionne. Le service doit être redémarré avant le prochain essai :

```bash
sudo fuser -k 69/udp
sudo target/debug/jupiter2ctl tftp-serve \
  --bind 0.0.0.0:69 \
  --root /home/mich/RustProjects/exec_modernise_v4/tftp-root
```

Le mot de passe sudo doit être saisi directement dans le terminal du PC. Cette
étape réseau est distincte du correctif `TMG0+0x0C` ; aucune nouvelle
conclusion vidéo ne doit être tirée tant que le binaire corrigé n'a pas produit
sa trace UART.

## Essai firmware corrigé — 17 octobre 2026

Le serveur TFTP corrigé a été lancé sur le PC Ryzen 5 et la Jupiter 2 a
transféré les deux fichiers depuis `192.168.1.131` :

```text
bonjour-jupiter2.bin       30861 octets
splashscreen-1600x900.bgra 5760000 octets
```

Le firmware a ensuite été lancé avec `go 0x140000000`. La trace UART confirme
le nouveau packing du timing horizontal :

```text
TMG0+0x0C_RB=0x00180000
TMG0+0x10_RB=0x00600050
TIMING_STAGE_OK
RDMA_STAGE_OK
DMMU_STAGE_OK
CMDLIST_STAGE_OK
AUX_STAGE_OK
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
MMIO_PROBE_OK
```

La sonde intermédiaire `AUX_AFTER_PLL` a signalé `AUX_AFTER_PLL_FAIL`, puis la
séquence AUX complète a réussi (`AUX_STAGE_OK`) et le lien a été entraîné. Ce
point reste à surveiller lors d'un prochain redémarrage.

Le lien DisplayPort et le pipeline de scanout sont donc initialisés par le
firmware. `SPLASH_OK` reste à déclarer après confirmation visuelle du splash
sur le moniteur HP Compaq LA2006x.

## 17 octobre 2026 — validation du chemin écran avec Bianbu

Après reset, U-Boot a démarré Bianbu depuis l'UFS avec `run ufs_boot`. Le
journal Linux confirme que le chemin physique utilisé par la Jupiter 2 est
fonctionnel avec l'adaptateur USB-C/HDMI :

```text
Product: USB Type-C Digital AV Adapter
spacemit-inno-dp-drv cac88000.dp1: DP: Training successful for R:1620000 L:2.
spacemit-inno-dp-drv cac88000.dp1: MSA: 1600x900, Rate:1620000 kHz, Lanes:2, BPP:24, TU:64.0
spacemit-drm-drv c0440000.display-subsystem-dp1: [drm] fb0: spacemitdrmfb frame buffer device
RISCV login:
```

Bianbu initialise donc le DP1 à `1600x900`, avec deux voies à `1,62 Gb/s`,
et expose le framebuffer au noyau. L'écran, l'adaptateur USB-C/HDMI et la
liaison physique sont validés par ce test. Le point restant pour V4 est le
flux bare-metal après l'initialisation `SCANOUT_STAGE_OK`; `SPLASH_OK` reste
réservé à l'observation visuelle du splash V4.

La validation visuelle a ensuite été confirmée : l'écran affiche effectivement
la page de connexion Bianbu. Le chemin complet Jupiter 2 → USB-C → HDMI → HP
Compaq LA2006x fonctionne donc avec le pilote Linux. Cette observation permet
d'isoler le défaut restant au pipeline vidéo bare-metal V4.

## 17 octobre 2026 — activation de scène DPU1 et strobe `SW_START`

Une première exécution du binaire reconstruit après le correctif `TMG0+0x0C`
s'est arrêtée à `DPU_SCENE_STAGE_BEGIN`. Trois appels `write_field()` avaient
encore leurs bornes dans l'ordre `low, high`, alors que la fonction attend
`high, low`. Les champs suivants ont été corrigés :

```text
DPU1+0x380 : bits 9:0
DPU1+0x38C : bits 7:2 et 13:8
```

Après reset et nouvelle exécution, la trace a atteint :

```text
DPU1+0x380_RB=0x01040001
DPU1+0x38C_RB=0x00000821
DPU_SCENE_STAGE_OK
AUX_AFTER_DPU_OK
LINK_TRAIN_OK
TMG0+0x00_ACTIVE_RB=0x00000184
DPU1+0x388_CFG_READY_RB=0x00000001
DPU1+0x390_SW_START_RB=0x00000000
```

Le zéro lu à `DPU1+0x390` correspond au comportement d'un registre de
commande à impulsion : le matériel efface `SW_START` après l'acceptation de la
commande. Le firmware traite maintenant `0` comme un readback valide pour ce
strobe. L'image mise à jour mesure `31053` octets et doit être exécutée après
un nouveau reset. `SCANOUT_STAGE_OK` et la confirmation visuelle du splash
restent à vérifier sur cette version.

## 17 octobre 2026 — exécution après correction du strobe

Après un nouveau reset, le binaire de `31053` octets a été transféré à
`0x140000000` et le splash 1600×900 à `0x102000000`. La trace UART complète
est arrivée jusqu'à :

```text
DPU_SCENE_STAGE_OK
AUX_AFTER_DPU_OK
LINK_TRAIN_OK
TMG0+0x00_ACTIVE_RB=0x00000184
DPU1+0x388_CFG_READY_RB=0x00000001
DPU1+0x390_SW_START_RB=0x00000000
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
MMIO_PROBE_OK
```

Le transfert TFTP, le chemin RDMA/DMMU/cmdlist, l'activation de scène DPU1,
le link training et l'activation du flux DP sont donc confirmés sur cette
version. Le `0` de `SW_START` est le readback attendu du strobe auto-effacé.
La validation restante est l'observation du moniteur; `SPLASH_OK` ne doit être
ajouté qu'après cette confirmation visuelle.

## 17 octobre 2026 — couche composite Saturn `layer00`

La trace complète précédente prouvait l'activation du chemin DPU/DP, mais elle
ne lisait pas `CMP1+0x38` après le déclenchement de la scène. La comparaison
avec `dpu_saturn_hee.c` et `composer_x.h` du pilote constructeur montre que le
commit Linux active la couche composite 0 avec la valeur :

```text
CMP1+0x38 layer00_ctrl = 0x0000FF03
```

Cette valeur active `layer00`, sélectionne RDMA0 et fixe l'alpha de couche à
`0xff`. Le firmware initialisait cette couche à zéro et la cmdlist n'avait
pas encore été vérifiée après `SW_START`. La nouvelle image écrit donc cette
valeur directement et publie `DPU_LAYER_STAGE_OK` après readback. Elle mesure
`31125` octets et doit être testée après reset; la confirmation visuelle du
splash reste nécessaire.

## 17 octobre 2026 — flux DP normal et diagnostics RDMA

La mire interne du transmetteur DP a été désactivée pour reproduire la
séquence Linux `soc_dp_hw_set_msa_and_enable_video()`. `DP1+0x238` reçoit
maintenant `VID_BIST_EN=0` ; le flux envoyé au moniteur doit donc provenir du
framebuffer BGRA chargé à `0x102000000`, via RDMA0, Saturn et le compositeur.
La configuration `TMG0+0x08=0x00002000` reste celle du format RGB888 du pilote
constructeur, et `DP1+0x200` conserve l'activation du flux DP normal.

Après `SW_START`, l'image publie aussi les diagnostics read-only du chemin de
scanout :

```text
DPU1+0x70C_IRQ_MASK       masque IRQ online
DPU1+0x730_IRQ_STATUS     événements online du compositeur 1
RDMA0+0xAC_DBG_RAW        causes brutes du debug RDMA0
RDMA0+0xB0_DBG_MASK       masque du debug RDMA0
RDMA0+0xB4_DBG_STATUS     statut actif du debug RDMA0
RDMA0+0xBC..0xCC          cinq registres d'information RDMA0
```

La reconstruction a passé `cargo fmt --all -- --check`, `cargo test
--workspace` (142 tests), `git diff --check` et le script de build. L'image
`tftp-root/bonjour-jupiter2.bin` mesure `35461` octets.

Le port `/dev/ttyUSB0` est accessible et le serveur TFTP écoute sur UDP 69.
Une écoute UART de huit secondes n'a reçu qu'un octet nul, sans nouvelle trace
de firmware : la Jupiter est donc probablement encore dans la boucle `WFI` de
l'image précédente, ou n'a pas été réinitialisée. Il faut effectuer un reset
matériel, laisser U-Boot interrompu, puis charger et lancer cette image :

```text
setenv ipaddr 192.168.1.132
setenv serverip 192.168.1.131
setenv netmask 255.255.255.0
tftpboot 0x102000000 splashscreen-1600x900.bgra
tftpboot 0x140000000 bonjour-jupiter2.bin
go 0x140000000
```

La trace attendue contient `DP1+0x238_COLORBAR_RB=0x00000000`,
`DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK`, puis `SCANOUT_DIAG_OK`. La ligne
`SPLASH_OK` reste réservée à la confirmation visuelle sur le HP Compaq
LA2006x.

## 17 octobre 2026 — exécution V4 avec scanout normal

Après reset, U-Boot a de nouveau détecté le moniteur HP Compaq LA2006x en
`1600x900`, puis s'est arrêté sur `V4_BOOT_HALTED`. Le transfert direct du
fichier de splash de `5760000` octets perdait des ACK à des blocs variables
(`170`, `375`, `1187` et `1222` selon les essais). Le serveur TFTP a été rendu
plus robuste : retransmission toutes les 500 ms, vingt tentatives et port de
transfert UDP éphémère. U-Boot perdait encore certains paquets sur le gros
fichier.

Le splash a donc été découpé en 88 morceaux de 64 KiB, puis reconstitué en
mémoire par les commandes `tftpboot` successives. Les 5 760 000 octets ont été
chargés sans erreur à partir de `0x102000000`; le binaire `35461` octets était
à `0x140000000`.

La trace du lancement V4 est complète :

```text
DP1+0x238_COLORBAR_RB=0x00000000
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
DPU1+0x730_IRQ_STATUS=0x00000000
RDMA0+0xB4_DBG_STATUS=0x00000000
SCANOUT_DIAG_OK
MMIO_PROBE_OK
```

Le flux DP normal, le framebuffer BGRA, RDMA0, Saturn et le compositeur sont
donc actifs sans underrun DPU ni erreur RDMA signalée. `SPLASH_OK` reste en
attente de la confirmation visuelle sur le moniteur.

## 17 octobre 2026 — correction DP après sortie de reset

Le pilote constructeur `spacemit_inno_dp.c` applique
`ENHANCE_FRAMING_EN=1` après les resets du contrôleur DP. La première version
V4 écrivait ce champ avant `DP_SOFT_RESET`; le reset pouvait donc effacer le
framing amélioré alors que l'AUX et le link training continuaient à réussir.

La séquence V4 réapplique désormais, après la sortie de tous les resets DP,
les champs suivants de `DP1+0x018` :

```text
SCRAMBLER_DISABLE       = 0
ENHANCE_FRAMING_EN      = 1
DEFAULT_FAST_LINK_TRAIN = 0
SCALE_DOWN_MODE         = 0
```

La trace ajoute les lectures :

```text
DP1+0x018_CORE_RB=
DP1+0x018_CORE_ACTIVE_RB=
DP1+0x088_HPD_RB=
DP1+0x080_GENERAL_INT_RB=
DP1+0x084_GENERAL_MASK_RB=
DP1+0x08C_HPD_ENABLE_RB=
```

Le binaire reconstruit est `tftp-root/bonjour-jupiter2.bin` (`35597` octets).
La vérification UART sans reset n'a reçu aucune nouvelle trace, ce qui laisse
la carte dans la boucle `WFI` de l'image précédente. Pour exécuter ce correctif
sur la Jupiter 2 :

```text
reset matériel
setenv ipaddr 192.168.1.132
setenv serverip 192.168.1.131
setenv netmask 255.255.255.0
tftpboot 0x102000000 splashscreen-1600x900.bgra
tftpboot 0x140000000 bonjour-jupiter2.bin
go 0x140000000
```

`SPLASH_OK` ne sera ajouté qu'après observation effective de l'image sur le
HP Compaq LA2006x.

## 18 octobre 2026 — test d'un écran rouge minimal

Le test a été simplifié pour supprimer le splash, le texte et les shaders. Une
surface BGRA8 uniforme rouge a été générée en 1600×900 : chaque pixel contient
`00 00 FF FF` dans l'ordre mémoire B, G, R, A. Comme le transfert direct de
5 760 000 octets reste sensible aux pertes TFTP, cette surface est découpée en
88 morceaux de 64 KiB dans `tftp-root/red-chunks-64k/`.

Le firmware garde `VID_BIST_EN=0` et envoie le framebuffer rouge par le chemin
normal Saturn/RDMA. L'exécution réelle a publié :

```text
RED_FRAMEBUFFER_TEST_BEGIN
DP1+0x238_COLORBAR_RB=0x00000000
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SCANOUT_DIAG_OK
MMIO_PROBE_OK
```

Le binaire chargé à `0x140000000` mesure `35621` octets et les morceaux rouges
ont été chargés à partir de `0x102000000`. La confirmation visuelle d'une
surface rouge uniforme est encore nécessaire avant de déclarer `SPLASH_OK`.

## 18 octobre 2026 — mire interne DP et entrée DVI

Le test du framebuffer rouge n'ayant pas produit l'image attendue, le
firmware a été recompilé avec la mire interne du transmetteur DP. Ce mode
contourne complètement la lecture du framebuffer et Saturn/RDMA :

```text
DP_COLORBAR_TEST_BEGIN
DP1+0x238_COLORBAR_RB=0x00000001
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SCANOUT_DIAG_OK
MMIO_PROBE_OK
```

La Jupiter émet la mire par DisplayPort Alt Mode sur USB-C. L'adaptateur
USB-C/HDMI doit ensuite fournir une liaison HDMI vers DVI-D compatible avec
l'entrée sélectionnée du HP Compaq LA2006x. Une absence de signal sur l'entrée
DVI avec cette mire oriente le diagnostic vers le câble, l'adaptateur, la
conversion HDMI-DVI ou la sélection d'entrée du moniteur.

La mire interne a ensuite été réellement exécutée après reconfiguration du
réseau U-Boot et transfert du binaire de `35621` octets. La trace UART confirme
`DP_COLORBAR_TEST_BEGIN`, `DP1+0x238_COLORBAR_RB=0x00000001`,
`LINK_TRAIN_OK`, `DP_MSA_STAGE_OK` et `SCANOUT_DIAG_OK`. Le HP Compaq reste
cependant sans signal sur son entrée DVI.

L'utilisateur observe désormais un écran bleu avec la mire interne. Cette
observation valide visuellement la chaîne Jupiter 2 → DisplayPort Alt Mode
USB-C → adaptateur HDMI → liaison DVI → HP Compaq LA2006x. Le transmetteur DP,
la conversion physique et l'entrée DVI fonctionnent. Le diagnostic peut donc
revenir au chemin Saturn/RDMA et au format du framebuffer BGRA, qui produisait
auparavant du noir au lieu du rouge.

## 17 octobre 2026 — exécution du correctif DP post-reset

Après reset, U-Boot a de nouveau détecté le HP Compaq LA2006x en `1600x900`.
Les 88 morceaux de 64 KiB ont été chargés à partir de `0x102000000`, puis le
binaire de `35597` octets a été transféré à `0x140000000` et exécuté.

La trace UART confirme que le framing amélioré est bien réappliqué après les
resets DP et reste actif pendant le flux vidéo :

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

`DP1+0x088_HPD_RB` indique que le HPD est présent. Le link reste valide après
l'activation vidéo et les diagnostics ne signalent ni underrun DPU ni erreur
RDMA. La confirmation visuelle du splash est encore nécessaire avant d'ajouter
`SPLASH_OK`.

## 17 octobre 2026 — état en pause

La session est mise en pause après l'exécution du correctif DP post-reset. Le
HP Compaq LA2006x reste sans signal, alors que la Jupiter 2 fournit les preuves
suivantes :

```text
DP1+0x018_CORE_ACTIVE_RB=0x40000000
DP1+0x088_HPD_RB=0x27800000
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SCANOUT_DIAG_OK
```

Les statuts DPU/RDMA ne signalent aucun underrun. Le chemin matériel est donc
initialisé jusqu'au flux DP normal, mais l'affichage visible reste à établir.
La prochaine étape est un essai contrôlé avec `VID_BIST_EN=1`, la mire interne
du transmetteur DP. Cette mire permettra de séparer le problème du chemin
Saturn/RDMA/framebuffer de celui du flux DP ou de l'adaptateur USB-C/HDMI.
`SPLASH_OK` ne doit être ajouté qu'après observation réelle de l'image.

## 18 octobre 2026 — cycle framebuffer bleu rouge vert

La mire interne a été retirée pour tester le framebuffer réel. Le firmware
remplit directement la surface BGRA 1600×900 et la fait cycler avec une pause
visible entre chaque couleur :

```text
RGB_CYCLE_TEST_BEGIN
RGB_CYCLE_BLUE
RGB_CYCLE_RED
RGB_CYCLE_GREEN
RGB_CYCLE_DONE
```

La trace UART confirme toujours `LINK_TRAIN_OK`, `DP_MSA_STAGE_OK`,
`SCANOUT_DIAG_OK` et `MMIO_PROBE_OK`. L'écran doit afficher successivement du
bleu, du rouge et du vert ; l'observation visuelle reste le contrôle final du
chemin Saturn/RDMA.

## 18 octobre 2026 — mire compositeur sans framebuffer

Le cycle framebuffer n'a produit aucun signal, alors que la mire DP interne a
affiché du bleu. Le prochain binaire active `SATURN_SOLID_TEST` : `CMP1`
génère directement une couleur pleine RGB 10 bits dans `layer00`, tandis que
le canal RDMA normal est désactivé. Le firmware fera ensuite passer cette
couche au bleu, au rouge puis au vert, sans lecture du framebuffer DDR.

Le binaire préparé est `tftp-root/bonjour-jupiter2.bin` (36 365 octets). Après
reset, il suffit de charger le firmware au prompt U-Boot :

```text
setenv ipaddr 192.168.1.132
setenv serverip 192.168.1.131
setenv netmask 255.255.255.0
tftpboot 0x140000000 bonjour-jupiter2.bin
go 0x140000000
```

La trace attendue contient `SATURN_SOLID_TEST_BEGIN` et les marqueurs
`RGB_CYCLE_SOLID_*_OK`. Ils valident les écritures de registres ;
`SPLASH_OK` reste réservé à une couleur réellement visible.

### Exécution de la mire compositeur

Après reset, le firmware a été chargé par TFTP à `0x140000000` puis lancé avec
`go 0x140000000`. La trace réelle confirme :

```text
DPU1+0x380_RB=0x01040000
CMP1+0x38_ACTIVE_RB=0x0000FF09
DPU_LAYER_STAGE_OK
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
RGB_CYCLE_SOLID_BLUE_OK
RGB_CYCLE_SOLID_RED_OK
RGB_CYCLE_SOLID_GREEN_OK
```

Le bit RDMA est bien désactivé pendant cette exécution et `layer00` du
compositeur est en mode couleur pleine. Les écritures Saturn sont donc
validées par UART. La validation visuelle du moniteur reste à relever avant
d'ajouter `SPLASH_OK`.

## 18 octobre 2026 — bleu DP reproduit

Pour vérifier que le résultat bleu observé précédemment était reproductible,
le firmware a été reconstruit avec `DP_COLORBAR_TEST`. La mire interne du
transmetteur Inno DP est maintenue active et le firmware reste ensuite en
attente, sans revenir au cycle Saturn.

Après reset, la trace UART réelle confirme :

```text
DP_COLORBAR_TEST_BEGIN
DP1+0x238_COLORBAR_RB=0x00000001
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
DP_COLORBAR_BLUE_HOLD
```

Le moniteur affiche effectivement un écran bleu. La chaîne physique et le
transmetteur DP sont donc reproductibles. Le défaut restant concerne
l'acheminement d'une image produite par Saturn vers le flux DP ; cette mire
interne ne valide pas le framebuffer ni le compositeur.

## 18 octobre 2026 — splash transféré en DRAM

Le fichier `splashscreen-1600x900.bgra` a été transféré par TFTP depuis le PC
Ryzen 5 vers la Jupiter 2 :

```text
Load address: 0x102000000
Bytes transferred = 5760000 (57e400 hex)
```

Le fichier est donc présent en DRAM à l'adresse prévue. Le transfert U-Boot ne
déclenche pas l'affichage à lui seul ; le firmware actuellement utilisé reste
configuré sur la mire bleue interne du transmetteur DP.

## 18 octobre 2026 — exécution du flux Saturn/RDMA

Après le transfert du splash à `0x102000000`, le firmware a été reconstruit
avec `SATURN_SOLID_TEST=0` et `DP_COLORBAR_TEST=0`, puis lancé à
`0x140000000`. La trace réelle confirme l'activation de la couche RDMA et la
désactivation de la mire DP :

```text
DPU1+0x380_RB=0x01040001
CMP1+0x38_ACTIVE_RB=0x0000FF03
DP1+0x238_COLORBAR_RB=0x00000000
RGB_CYCLE_BLUE
RGB_CYCLE_RED
RGB_CYCLE_GREEN
RGB_CYCLE_DONE
```

Le flux Saturn/RDMA est donc exécuté avec le splash en mémoire. La trace seule
ne suffit pas à déclarer `SPLASH_OK` ; l'image effectivement visible doit
encore être relevée.

## 18 octobre 2026 — correction des horloges APMU et mire DP de référence

Le relevé Linux effectué après `run ufs_boot` donne pour le DPU1 :

```text
dsi4ln2_lcd_pxclk  107142857 Hz   # mode DP : PLL réglé à 108000 kHz
dsi4ln2_lcd_mclk   307200000 Hz
dsi4ln2_dpu_aclk   409600000 Hz
```

Le firmware écrivait des bits PXCLK/MCLK dans `LCD_CLK_RES_CTRL3`
(`APMU+0x26C`), alors que ce registre ne contient que l'ESC et le reset
logiciel LCD. Le correctif conserve dans `CTRL3` les seuls bits ESC, active et
programme PXCLK/MCLK dans `CTRL4` (`APMU+0x270`), puis programme la sélection
et le diviseur ACLK dans `CTRL5` (`APMU+0x274`). Les valeurs appliquées
correspondent à l'arbre Linux : PXCLK source 5, diviseur 4 ; MCLK source 2,
diviseur 1 ; ACLK source 0, diviseur 0.

La reconstruction et le transfert U-Boot ont produit `36477` octets. La trace
du flux Saturn/RDMA corrigé contient `APMU_ENABLE_OK`, `LINK_TRAIN_OK`,
`DP_MSA_STAGE_OK`, `SCANOUT_STAGE_OK` et
`DP1+0x238_COLORBAR_RB=0x00000000`. Un second binaire de diagnostic a
reproduit la référence indépendante de Saturn :

```text
DP1+0x238_COLORBAR_RB=0x00000001
DP_COLORBAR_BLUE_HOLD
```

Le dépôt est revenu au mode normal (`DP_COLORBAR_TEST=false`) après cette
vérification. `SPLASH_OK` reste réservé à l'observation visuelle du splash.

## 18 octobre 2026 — watchdog hérité d'U-Boot identifié

La mire bleue apparaissait puis disparaissait après un délai régulier. La
capture UART du redémarrage a fourni la cause : U-Boot affiche
`WDT: Started watchdog@d4014000 with servicing (60s timeout)`, puis cesse de
rafraîchir le watchdog lorsqu'il transfère le contrôle au firmware par `go`.
Le firmware restait ensuite dans sa boucle de test vidéo et le watchdog
réinitialisait toute la carte.

Le firmware désactive maintenant WDT0 dès l'initialisation de l'UART, en
respectant les clés documentées `TWFAR=0xBABA` et `TWSAR=0xEB10`, puis écrit
`TWER=0`. Il publie `WDT0+0x0B8_RB=...` et `WDT_DISABLED` sur l'UART. Le test
de référence est revenu à la mire bleue interne du transmetteur DP
(`DP_COLORBAR_TEST=true`) afin de vérifier que cette mire reste stable sans
dépendre de Saturn, du RDMA ou de la lecture DDR.

Image reconstruite : `tftp-root/bonjour-jupiter2.bin`, 35837 octets. La
validation matérielle de cette nouvelle image doit confirmer la trace
`WDT_DISABLED` et l'absence de redémarrage au-delà de 60 secondes.

## 18 octobre 2026 — mire bleue stable après correction du watchdog

La nouvelle image a été transférée à `0x140000000` puis lancée par U-Boot.
La trace UART contient :

```text
WDT0+0x0B8_RB=0x00000000
WDT_DISABLED
DP1+0x238_COLORBAR_RB=0x00000001
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
DP_COLORBAR_BLUE_HOLD
```

La capture est restée active au-delà de 60 secondes sans nouvelle séquence
SPL/U-Boot. Le redémarrage qui faisait disparaître la mire était donc bien
causé par le watchdog hérité d'U-Boot ; la chaîne DP de référence reste
active. Le prochain essai peut revenir au chemin Saturn/RDMA sans réintroduire
ce redémarrage parasite.

La mire bleue a été observée directement sur le moniteur. La chaîne physique
Jupiter 2 → DisplayPort → adaptateur USB-C/HDMI → écran HP Compaq LA2006x est
ainsi validée avec le firmware bare-metal, et sa stabilité au-delà du délai
du watchdog est confirmée.

## 18 octobre 2026 — correction du handshake scène DPU1

La mire Saturn pleine a bien exécuté ses écritures et son cycle bleu-rouge-vert
sur l'UART, mais aucune couleur n'a été visible. La comparaison avec le pilote
constructeur a révélé que le handshake de démarrage utilisait le mauvais bloc
de registres : le firmware écrivait `CFG_READY` et `SW_START` à
`DPU1+0x388/0x390`, dans le bloc scène 2. Pour DPU1, le pilote Linux utilise
`DPU_SCENE_CTL_ADDR(1)=0x340`, soit :

```text
DPU1+0x348  both_cfg_rdy
DPU1+0x350  sw_start
DPU1+0x34C  video_mod/timing_inter0/timing_inter1
```

Le firmware utilise maintenant ce bloc pour `nml_rch_en`, `nml_outctl_en`,
`nml_frm_timing_en`, le mode vidéo, `CFG_READY` et `SW_START`. Le binaire a
été reconstruit dans `tftp-root/bonjour-jupiter2.bin` et mesure `40645` octets.
La nouvelle trace doit contenir `DPU1+0x348_CFG_READY_RB` et
`DPU1+0x350_SW_START_RB`. La carte doit encore être réinitialisée et cette
image doit être chargée à `0x140000000` avant de conclure sur l'affichage.

## 18 octobre 2026 — premier flux visible et diagnostic couleur

Après reset, la nouvelle image de `40645` octets a été chargée puis exécutée.
Le handshake correct est confirmé par :

```text
DPU1+0x340_RB=0x01250000
DPU1+0x348_CFG_READY_RB=0x00000001
DPU1+0x350_SW_START_RB=0x00000000
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
RGB_CYCLE_SOLID_BLUE_OK
RGB_CYCLE_SOLID_RED_OK
RGB_CYCLE_SOLID_GREEN_OK
```

L'utilisateur observe maintenant un écran gris. C'est le premier retour visuel
depuis le lancement du chemin Saturn : le lien DP, le timing DPU et le flux
vidéo sont actifs, mais la couleur de la couche Saturn n'est pas encore
reproduite visuellement. Le prochain binaire désactive `layer00` et programme
le fond rouge du TMG (`TMG0+0x24/0x28`) afin de séparer le chemin couleur du
compositeur de celui de la sortie DP. Il mesure `35981` octets.

### Exécution du fond TMG

Après le second reset, l'image de `35981` octets a été transférée et exécutée.
La trace confirme le fond direct :

```text
TMG0+0x24_BG_RG_RB=0x00000FFF
TMG0+0x28_BG_B_RB=0x00000000
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
TMG_BACKGROUND_RED_HOLD
```

Le contrôle visuel de la couleur rouge reste à relever sur le moniteur.

La couleur rouge est effectivement visible. Le test isole donc le défaut à la
configuration de la couche Saturn. Le prochain firmware revient à Saturn avec
un fond TMG noir et active `layer00_solid_area_mode` dans
`CMP1+0x38` (`0x0001FF09`). Il mesure `40685` octets.

## 18 octobre 2026 — mire DP bleue reproduite après reset

Pour séparer définitivement la sortie physique du compositeur Saturn, la mire
interne du transmetteur DP a été réactivée dans un binaire de référence de
`36029` octets. Après reset, transfert TFTP et lancement à `0x140000000`, la
trace UART a confirmé :

```text
WDT_DISABLED
DP1+0x238_COLORBAR_RB=0x00000001
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
DP_COLORBAR_BLUE_HOLD
```

La mire bleue est visible sur le HP Compaq LA2006x. Le chemin Jupiter 2 → DP1
→ USB-C/HDMI → DVI → moniteur est donc reproductible. Le défaut restant est
limité à la production de pixels par Saturn (`CMP1`/`layer00` et RDMA) ; le
prochain essai désactive à nouveau la mire DP.

## 18 octobre 2026 — correction du registre `solid_area_mode`

Le test Saturn `layer00` reste noir malgré les readbacks corrects. La lecture
du pilote constructeur `composer_x.h` a localisé une erreur de registre :
`solid_area_mode` est le bit 16 de `CMP1+0x34`, avec `solid_color_a`, tandis
que `CMP1+0x38` doit rester à `0x0000FF09` pour activer la couche et sa couleur
pleine. Le firmware écrivait auparavant le bit 16 dans `CMP1+0x38`, où il
correspond à `alpha_ratio`.

Le correctif ajoute donc `0x00010000` à la valeur d'alpha écrite à `CMP1+0x34`
et conserve `CMP1+0x38=0x0000FF09`. Le binaire est reconstruit dans
`tftp-root/bonjour-jupiter2.bin` (`40685` octets). Le prochain essai doit
confirmer `CMP1+0x34_SOLID_A_RB=0x000100FF` et afficher le bleu solide de
`layer00`.

## 18 octobre 2026 — exécution de la correction `solid_area_mode`

Le binaire corrigé de `40685` octets a été reconstruit, transféré par TFTP à
`0x140000000`, puis exécuté après un redémarrage de la Jupiter 2. Le splash
1600×900 a été rechargé à `0x102000000` (`5760000` octets). La trace UART
confirme les registres corrigés :

```text
CMP1+0x34_SOLID_A_RB=0x000100FF
CMP1+0x38_ACTIVE_RB=0x0000FF09
DPU1+0x348_CFG_READY_RB=0x00000001
DPU1+0x350_SW_START_RB=0x00000000
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
RGB_CYCLE_SOLID_BLUE_OK
RGB_CYCLE_SOLID_RED_OK
RGB_CYCLE_SOLID_GREEN_OK
RGB_CYCLE_DONE
```

La carte n'a pas redémarré pendant l'essai ; `WDT_DISABLED` est confirmé. Le
chemin Saturn `CMP1/layer00` accepte donc maintenant le mode de couleur pleine
et les changements de scène. La confirmation visuelle sur le moniteur reste
le dernier contrôle de ce jalon.

## 18 octobre 2026 — relecture du pilote DPU Saturn HEE

La documentation K3 locale décrit les fonctions générales du DPU, mais pas tous
les champs de la variante Saturn HEE. Une relecture du pilote constructeur
public `spacemit-com/linux-6.18`, branche `k3-br-v1.0.y`, a confirmé les bases
utilisées par V4 et a trouvé deux erreurs dans la séquence bare-metal :

- `dpu_scene_ctl.h` place `timing_inter0` dans les bits `6:1` et
  `timing_inter1` dans les bits `12:7` du registre `DPU1+0x34C`. Le firmware
  écrivait auparavant les champs dans `7:2` et `13:8` ; le readback `0x821`
  confirmait donc une valeur mal encodée. La correction attend `0x411` pour
  les deux intervalles à 8 ;
- `saturn_get_cl_cfg_rdy_addr()` place `CMDLIST_CFG_READY` à
  `DPU_CTRL_TOP+0x34`, soit `DPU1+0x3F4`. `DPU1+0x534` est réservé dans la
  fenêtre cmdlist. Les deux bits d'activation du composer 1 sont à
  `DPU1+0x3E8` (`DPU_CTRL_TOP+0x28`), et le firmware écrivait auparavant à
  `0x3EC`.
- `wb_cmdlist()` charge la liste du compositeur sur le canal
  `rdma_nums + 2 + crtc_id`. Avec quatre RDMA et `crtc_id=1` pour DPU1, le
  canal correct est 7 ; l'ancien firmware chargeait le canal 0, celui de
  RDMA0.

La séquence HEE de référence confirme par ailleurs les adresses suivantes :
scene control `0x340`, control-top `0x3C0`, cmdlist `0x500`, RDMA0 `0x1000`,
TBU0 `0x1100`, CMP1 `0x30000` et TMG0 `0x51200`. Les corrections sont dans
`firmware/bonjour/src/main.rs`. Elles sont compilées localement ; un nouveau
reset et un transfert TFTP sont nécessaires avant toute conclusion visuelle.

## 18 octobre 2026 — exécution du correctif DPU et cmdlist canal 7

Le binaire corrigé de `35973` octets a été transféré par TFTP à
`0x140000000`, puis lancé depuis U-Boot. La trace UART a confirmé :

```text
CMDLIST7+0x1C_RB=0x00110000
CMDLIST7+0x5C_RB=0x00000001
CMDLIST7+0xDC_RB=0x00000000
CMDLIST_STAGE_OK
DPU1+0x3E8_RB=0x00000003
CMDLIST_ENABLE_OK
DPU1+0x34C_RB=0x00000411
DPU_SCENE_STAGE_OK
CMP1+0x30_SOLID_B_RB=0x000003FF
CMP1+0x34_SOLID_A_RB=0x000100FF
CMP1+0x38_ACTIVE_RB=0x0000FF09
DPU_LAYER_STAGE_OK
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SATURN_SOLID_BLUE_HOLD
```

Le chemin Saturn/DP est actif et maintenu en bleu solide. La validation
visuelle du moniteur reste le dernier contrôle de ce jalon.

## 18 octobre 2026 — échec final du flux Saturn/compositeur

Le test suivant a conservé le flux DP normal, désactivé la mire DP interne et
programmé un fond rouge directement dans `CMP1`. La configuration courante
était :

```text
DP_COLORBAR_TEST=false
TMG_BACKGROUND_TEST=false
CMP_BACKGROUND_TEST=true
SATURN_SOLID_TEST=true
```

Le binaire courant mesurait `36037` octets. L'UART a confirmé la configuration
du fond et le maintien du firmware :

```text
CMP1+0x08_BG_R_RB=0x000003FF
CMP1+0x0C_BG_G_RB=0x00000000
CMP1+0x10_BG_B_RB=0x00000000
CMP1+0x38_ACTIVE_RB=0x00000000
CMP_BACKGROUND_RED_HOLD
```

Malgré cette trace, l'utilisateur a observé un écran noir. Le test est donc
classé **échec d'affichage** : les écritures et les readbacks du compositeur
ne suffisent pas à produire des pixels visibles dans le flux Saturn normal.

Les points suivants restent validés et ne doivent pas être réexaminés en
premier lors de la reprise :

- Bianbu affiche son écran de connexion avec le HP Compaq LA2006x et le même
  adaptateur USB-C/HDMI ;
- le fond rouge généré directement par TMG a été visible ;
- la mire DP interne bleue a été visible et est restée stable après la
  désactivation du watchdog U-Boot ;
- `LINK_TRAIN_OK`, `DP_MSA_STAGE_OK` et `SCANOUT_STAGE_OK` indiquent que la
  séquence de contrôle DP s'exécute, mais ne constituent pas une preuve de
  pixels affichés.

La panne restante est donc localisée au chemin de composition DPU1/Saturn
(`CMP1`, scène, cmdlist, RDMA/DMMU ou routage vers TMG). Le splash BGRA n'est
pas validé et `SPLASH_OK` reste volontairement absent. La session est arrêtée
à ce point ; aucune nouvelle commande ni aucun nouveau reset n'est requis.

## 18 octobre 2026 — essai instrumenté bloqué par HPD absent

Une sonde en lecture seule a été ajoutée après `SW_START` pour distinguer les
valeurs simplement écrites des valeurs conservées par le compositeur actif.
Le binaire de `36461` octets a été transféré avec succès par TFTP et lancé à
`0x140000000`. Avant la partie DP, la trace confirme :

```text
RDMA0+0x3C_RB=0x00001900
TBU0+0x28_RB=0x0000057F
RDMA0+0x24_VA_RB=0x10000000
RDMA0+0x28_VA_RB=0x00000000
DMMU_STAGE_OK
CMDLIST_STAGE_OK
DPU_SCENE_STAGE_OK
DPU_LAYER_STAGE_OK
```

Le firmware n'a pas atteint le nouveau bloc `DPU_POST_COMMIT`, car le HPD
n'était pas présent au moment du reset :

```text
dp cannot get HPD signal
AUX_AFTER_DPU_FAIL
AUX_AFTER_PLL_FAIL
DPCD_REV=0x00000000
DPCD_RATE=0x00000000
DPCD_LANES=0x00000000
AUX_STAGE_READBACK_FAIL
```

Cet essai est classé **interrompu par absence de HPD**, et non comme un nouvel
échec du compositeur. L'écran doit être relié à la Jupiter 2 et placé sur la
bonne entrée avant le prochain reset ; la sonde post-commit pourra ensuite
fournir les valeurs effectives de `CMP1`, de la scène DPU1 et de la cmdlist.

## 18 octobre 2026 — vrai test Saturn bleu après reset

Le diagnostic `CMP_BACKGROUND_TEST` était encore actif lors de l'essai du
fond rouge. Cette option désactive `layer00`, ce qui explique le readback
`CMP1+0x38_ACTIVE_RB=0x00000000` de cette trace. Elle a été désactivée pour
tester réellement la couche pleine Saturn.

Après un reset effectivement visible dans l'UART, le binaire de `36405`
octets a été transféré par TFTP à `0x140000000`. La mémoire a été vérifiée
avant l'exécution :

```text
140000000: 17 91 01 00 13 01 01 00 97 10 00 00 e7 80 c0 06
Bytes transferred = 36405 (8e35 hex)
```

La trace complète atteint le commit post-scène avec la couche bleue active :

```text
CMP1+0x30_SOLID_B_RB=0x000003FF
CMP1+0x34_SOLID_A_RB=0x000100FF
CMP1+0x38_ACTIVE_RB=0x0000FF09
TMG0+0x00_ACTIVE_RB=0x00000181
DPU1+0x348_POST_RB=0x00000000
DPU1+0x350_POST_RB=0x00000000
DPU_POST_COMMIT_END
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SATURN_SOLID_BLUE_HOLD
```

`CFG_READY` et `SW_START` retombent à zéro après consommation par le matériel,
tandis que les valeurs effectives de `CMP1` restent présentes. Le lien DP est
validé (`LINK_TRAIN_OK`) et aucune IRQ d'erreur DPU/RDMA n'est signalée. Cette
exécution constitue le test Saturn bleu correct ; il reste à confirmer la
couleur observée sur le HP Compaq LA2006x.

## 18 octobre 2026 — instrumentation explicite du test Saturn direct

Le firmware a ensuite été recompilé avec des marqueurs UART qui indiquent
directement le chemin actif. Le binaire chargé par TFTP contient `36501` octets.
Après le reset, la trace a fourni :

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

Dans cette variante, `SATURN_SOLID_DIRECT_TEST` vaut `true`. Le canal RDMA
reste désactivé et le registre de cmdlist compositeur est explicitement mis à
zéro :

```text
DPU1+0x3C0_RB=0x00000000
DPU1+0x3E8_RB=0x00000000
RDMA0+0xB4_POST_RB=0x00000000
```

La suite de contrôle DP atteint toujours le commit et le maintien final :

```text
LINK_TRAIN_OK
DPU_POST_COMMIT_END
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SATURN_SOLID_BLUE_HOLD
```

`AUX_AFTER_PLL_FAIL` reste un échec de lecture intermédiaire immédiatement
suivi par la séquence AUX normale (`AUX_STAGE_OK`), puis par
`LINK_TRAIN_OK`. Aucun défaut DPU/RDMA n'est signalé dans cette exécution.
Cette trace isole donc le prochain contrôle à effectuer : vérifier la couleur
réellement reçue par le moniteur, puis comparer avec une variante où la cmdlist
Saturn est réactivée. Le splash BGRA reste hors de ce test minimal.

## 18 octobre 2026 — correction du test direct Saturn sans pixels

La variante `SATURN_SOLID_DIRECT_TEST=true` n'affichait rien parce qu'elle
mettait volontairement `DPU1+0x3E8` à zéro. Elle écrivait les champs `CMP1` par
MMIO, mais désactivait au même moment la cmdlist compositeur HEE ; les
readbacks `CMP1` ne constituaient donc pas une preuve de scène active.

Le firmware a été corrigé avec `SATURN_SOLID_DIRECT_TEST=false`, ce qui réactive
la soumission de la couche par la cmdlist constructeur. Après reset, TFTP et
exécution, l'UART confirme :

```text
SATURN_MODE=SOLID_CMDLIST
DPU1+0x3E8_RB=0x00000003
DPU1+0x340_RB=0x01040000
DPU1+0x34C_RB=0x00000411
CMP1+0x38_ACTIVE_RB=0x0000FF09
DPU1+0x3E8_POST_RB=0x00000003
LINK_TRAIN_OK
SCANOUT_STAGE_OK
SATURN_SOLID_BLUE_HOLD
```

La couche bleue est désormais envoyée par le chemin HEE utilisé par le pilote
Linux. Si l'écran reste noir malgré cette variante, la prochaine sonde devra
lire l'état de reload/consommation de la cmdlist et du compositeur, car les
readbacks des registres d'écriture sont alors insuffisants.
