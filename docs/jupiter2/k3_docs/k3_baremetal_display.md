# K3 — portage bare-metal de l'affichage

Ce document conserve les faits utilisés par le bring-up V4 de la Jupiter 2.
Les adresses proviennent du device tree K3 Pico-ITX et les séquences du pilote
DRM SpacemiT Linux 6.18 `k3-br-v1.0.y`.

## Chemin actif

Le Pico-ITX relie la sortie externe à `dp1`/eDP1 :

| Bloc | Adresse | Taille DT | Rôle |
|---|---:|---:|---|
| DPU1 Saturn | `0xC0440000` | `0x54000` | CRTC/planes |
| DP1/eDP1 | `0xCAC88000` | `0x4000` | émetteur DisplayPort |
| APMU | `0xD4282800` | contrôleur système | clocks/resets |
| framebuffer test | `0x102000000` | `8294400` | BGRA8 1920×1080 |

Les registres APMU utiles au premier relevé sont :

| Registre | Adresse | Éléments concernés |
|---|---:|---|
| `LCD_EDP_CTRL` | `APMU + 0x23C` | horloge pixel et reset EDP1 |
| `LCD_CLK_RES_CTRL3` | `APMU + 0x26C` | horloge ESC et reset LCD logiciel |
| `LCD_CLK_RES_CTRL4` | `APMU + 0x270` | horloges MCLK/DSC et leurs resets |
| `LCD_CLK_RES_CTRL5` | `APMU + 0x274` | horloge DPU ACLK et son reset |

Le firmware de diagnostic les lit après les fenêtres DPU1/DP1 et publie les
valeurs brutes sur l'UART. L'étape suivante fait une lecture-modification-
écriture limitée aux gates d'horloge et aux bits de désassertion de reset
identifiés par le pilote Linux, avec lecture de retour immédiate.

U-Boot a détecté un sink DPCD 1.2 à 5,4 Gb/s et deux voies. La sonde V4
lecture seule a obtenu `DPU1+0x000 = 0x08040030` et `DP1+0x00C =
0x02A1350D`. Elle a également lu zéro aux offsets DPU1 `0x340`, `0x380`,
`0x3C0` et DP1 `0x000`, ce qui correspond à une sortie non configurée après
l'échec d'initialisation display d'U-Boot.

## Ordre de portage

Le pilote Linux déclare pour DPU1 le power-domain `K3_PMU_LCD1_PWR_DOMAIN`,
les horloges `DSI4LN2_LCD_PXCLK`, `DSI4LN2_LCD_MCLK`, `DSI4LN2_DPU_ACLK`,
`DSI4LN2_DSI_ESC` et `DSI4LN2_LCD_DSC`, ainsi que les cinq resets associés.
DP1 ajoute l'horloge `EDP1_PXCLK` et le reset `EDP1`.

Après cette mise sous tension, le portage doit reproduire les quatre blocs
Linux suivants :

1. initialisation des registres Saturn, DMA top, MMU top et interruptions ;
2. table DMMU du plane et configuration RDMA pour `BGRA8888`, largeur 1920,
   hauteur 1080 et stride 7680 ;
3. cmdlist Saturn avec adresse physique et configuration de scène/timing ;
4. reset DP1, PLL pixel à 148500 kHz, deux voies à 2,70 Gb/s, AUX link
   training, MSA 1920×1080 puis activation du flux vidéo.

Le pilote Linux utilise aussi un reset matériel et des GPIO de puissance,
d'activation et de rétroéclairage pour eDP. Ces signaux devront être
identifiés dans le DTB de la carte avant l'activation bare-metal. Aucun de ces
signaux n'est encore piloté par `firmware/bonjour` à ce stade.

## Preuve V4

Les marqueurs actuels sont :

```text
UART_OK
DRAM_OK
SPLASH_RAM_READ
MMIO_PROBE_OK
```

`MMIO_PROBE_OK` signifie seulement que les lectures MMIO ont abouti. Le
marqueur `SPLASH_OK` sera ajouté uniquement après validation du link training,
du scanout et de l'image visible.

## Preuve APMU

Le 17 octobre 2026, les lectures U-Boot puis bare-metal ont concordé :

```text
APMU+0x23C = 0x00070000
APMU+0x26C = 0x0080401C
APMU+0x270 = 0x01A9C343
APMU+0x274 = 0x00000003
```

La lecture U-Boot correspondante était `0xD4282A3C`, `0xD4282A6C`,
`0xD4282A70` et `0xD4282A74`. Le firmware a publié `APMU_PROBE_OK` après ces
lectures. L'image de diagnostic ajoute une étape `APMU_ENABLE_BEGIN` qui ne
modifie que les gates et resets DPU1/eDP1, signale `APMU_ENABLE_OK` après
vérification de lecture, puis s'arrête encore avant toute écriture Saturn,
DMMU ou DP.

Cette écriture a été validée sur la carte :

```text
APMU+0x26C_RB = 0x0081401D
APMU+0x270_RB = 0x01A9C343
APMU+0x274_RB = 0x00000003
APMU+0x23C_RB = 0x00070000
APMU_ENABLE_OK
```

Le changement observé dans `CTRL3` ajoute les gates MCLK et PXCLK. Les autres
bits ciblés étaient déjà actifs et ont été conservés par la lecture-
modification-écriture. Le power-domain, le pixel-rate final et les blocs
Saturn/DMMU/RDMA restent à porter.

L'initialisation minimale Saturn a ensuite été validée pour le pipeline 1 :

```text
DPU1+0x1284 = 0x00000810
DPU1+0x1288 = 0xFFFFFFFE
DPU1+0x1200 = 0xFFFF0000
DPU1+0x1208 = 0x00000008
DPU1+0x120C = 0x00000008
DPU1+0x30000 = 0x00000001
DPU_REG_INIT_OK
```

Le composer est activé sans couche. Le timing, le plane RDMA, la table DMMU,
la cmdlist et le contrôleur DP restent à initialiser avant de déclarer
`SPLASH_OK`.

Le timing Saturn a maintenant été préparé pour DP1 sans activation du flux :

```text
TMG0 = DPU1 + 0x51200
1920x1080@60 : hfp=88 hsync=44 hbp=148 vfp=4 vsync=5 vbp=36
TMG0+0x00 = 0x00000180       # hsp/vsp, fm_timing_en=0
TMG0+0x08 = 0x00002000       # RGB888
TMG0+0x0C = 0x00940058       # hbp/hfp
TMG0+0x10 = 0x0094002C       # hbp/hsync
TMG0+0x14 = 0x00050004       # vsync/vfp
TMG0+0x18 = 0x07800024       # hactive/vbp
TMG0+0x1C = 0x00000438       # vactive
TMG0+0x3C = 0x00000001       # vsync align with hsync
```

La carte a confirmé `TIMING_STAGE_OK` sur les lectures de retour. Le plane
RDMA, la table DMMU, la cmdlist, le PLL pixel et le contrôleur DP restent à
initialiser avant de déclarer `SPLASH_OK`.

Le plane a ensuite été préparé sans activation :

```text
RDMA0+0x00 = 0x00002000       # composer 1
RDMA0+0x24 = 0x02000000       # adresse basse du framebuffer
RDMA0+0x28 = 0x00000001       # adresse haute, soit 0x102000000
RDMA0+0x3C = 0x00001E00       # stride 7680
RDMA0+0x40 = 0x04380780       # 1920 x 1080
RDMA0+0x44 = 0x00000000       # bbox début
RDMA0+0x48 = 0x0437077F       # bbox fin
RDMA0+0x74 = 0x00000007       # BGRA8888
CMP1+0x04  = 0x04380780       # destination composer
CMP1+0x38  = 0x00000000       # couche 0 désactivée
```

La carte a confirmé `RDMA_STAGE_OK`. Cette étape valide la représentation
physique et le format du framebuffer ; le DMMU et la cmdlist restent
nécessaires avant toute lecture par RDMA.

## AUX et link training validés

Le connecteur `dp1` est le DisplayPort externe du Pico-ITX (`dp-id = 1`). Il
ne faut pas activer `ENABLE_EDP` dans `DP1+0x18`. Avec le switch vidéo placé sur
l'entrée Jupiter, U-Boot signale un HPD actif et lit un sink DPCD 1.2 avec un
débit maximal de 5,4 Gb/s et deux voies.

Les registres AUX sont des registres de commande. La séquence bare-metal qui
fonctionne est une écriture complète de `AUX_CONFIG`, puis `AUX_START=0` et
`AUX_START=0x80000000`, suivie d'une attente de l'événement bit 16. Une remise
à zéro AUX avant le premier échange est nécessaire après l'état laissé par
U-Boot. La lecture DPCD validée est :

```text
DPCD_REV=0x00000012
DPCD_RATE=0x00000014
DPCD_LANES=0x000000C2
AUX_INT_RB=0x00010000
AUX_STATUS_RB=0x0004B149
AUX_CONFIG_RB=0x0F000009
AUX_STAGE_OK
```

Le link training HBR 2,70 Gb/s sur deux voies utilise TPS1 puis TPS3 et
réinjecte les demandes de swing/préaccentuation du sink. La carte a confirmé
la récupération d'horloge puis l'égalisation :

```text
LINK_CR_STATUS=0x00000000
LINK_CR_STATUS=0x00000000
LINK_CR_STATUS=0x00000011
LINK_EQ_STATUS=0x00000077
LINK_TRAIN_OK
```

Le MSA 1920×1080 RGB888 est ensuite programmé avec le flux vidéo DP. Le timing
Saturn passe de `0x00000180` à `0x00000181` et la capture UART publie
`DP_MSA_STAGE_OK` puis `SCANOUT_STAGE_OK`. Cette preuve valide le chemin
matériel jusqu'au scanout ; `SPLASH_OK` attend encore la confirmation visuelle
du framebuffer BGRA sur le moniteur.

Le premier essai avec l'écran relié au switch n'a toutefois affiché aucun
splash. Le prochain essai se fera avec le HP Compaq LA2006x. Son mode natif est
probablement 1600×900, tandis que le firmware utilise encore un mode fixe
1920×1080. La lecture de l'EDID et la sélection automatique du mode devront
précéder une nouvelle conclusion sur le scanout. Tant que l'image n'est pas
visible, `SPLASH_OK` reste volontairement absent.

## Diagnostic sans signal

Le HP Compaq LA2006x directement relié à DP1 indique toujours « pas de
signal », malgré l'AUX et le link training validés. Trois points du portage
restent vérifiés dans le prochain binaire :

1. `PHY_BUSY_BYP` est conservé à 1 après l'entraînement, comme dans
   `spacemit_inno_dp.c` ;
2. la table DMMU utilise 1407 pages pour le framebuffer 1600×900 BGRA8, avec
   les 60 entrées de garde requises par Saturn ;
3. la cmdlist encode les événements reload RDMA (bit 4) et composer (bit 6)
   dans ses deux mots d'attente avant `CFG_RDY`.

La version préparée n'est pas encore une preuve matérielle : elle doit être
transférée puis exécutée après un reset de la carte.

## Exécution du correctif cmdlist

Après reset, le transfert et l'exécution du binaire corrigé ont abouti sur la
carte :

```text
DMMU_TABLE_FIRST=0x00102000
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

La valeur `0x80060121` confirme que `PHY_BUSY_BYP` reste actif après le
training. Le dernier jalon prouve l'activation matérielle du scanout ; il ne
prouve pas encore que le framebuffer est visible. `SPLASH_OK` attend donc la
lecture visuelle du moniteur.
## Essai DMT 1600×900@60

Le moniteur HP Compaq LA2006x est confirmé en 1600×900 à 60 Hz. Pour ce
moniteur, le prochain firmware utilise le minutage DMT standard : pixel clock
108 MHz, porches horizontaux 24/80/96, porches verticaux 1/3/96 et synchros
80/3. Le PLL pixel utilise l'entrée Linux 108 MHz (`fbdiv=117`, `divaux=13`).
Cette modification est préparée et compilée ; elle ne sera considérée comme
validée qu'après une nouvelle exécution UART et un signal vidéo visible.

## Validation UART du mode DMT

Le mode DMT 1600×900@60 a été exécuté sur la Jupiter 2. Le framebuffer BGRA8
1600×900 a été chargé à `0x102000000`, puis le firmware a programmé le PLL pixel
108 MHz et les porches `24/80/96` horizontaux et `1/3/96` verticaux.

La séquence réelle a confirmé `DMMU_STAGE_OK`, `CMDLIST_ENABLE_OK`,
`LINK_TRAIN_OK`, `DP_MSA_STAGE_OK` et `SCANOUT_STAGE_OK`. Ces marqueurs valident
les écritures et les lectures de retour du chemin bare-metal ; le marqueur
`SPLASH_OK` attend encore l'observation du framebuffer sur le moniteur.

## Chaîne USB-C/HDMI

Le chemin physique de test est USB-C avec adaptateur HDMI. DP1 est le
transmetteur DisplayPort source de la Jupiter ; l'adaptateur convertit le
DisplayPort Alt Mode en HDMI. Une trace DPCD ou de link training positive ne
suffit donc pas, à elle seule, à certifier le signal HDMI au moniteur.

## Correction du champ HFP Saturn

La comparaison avec `drivers/gpu/drm/spacemit/dpu/dpu_saturn_hee.c` du noyau
K3 a identifié une erreur dans le premier portage du timing. Le registre
`TMG0+0x0C` contient `split_overlap` dans les bits `9:0` et `HFP` dans les bits
`27:16`. `HBP` est dans le registre suivant, `TMG0+0x10`.

Pour 1600×900 DMT, l'ancienne écriture `0x00600018` encodait donc un
chevauchement de 24 pixels et un HFP de 96 pixels. Le correctif écrit
`HFP << 16`, donc `0x00180000`, tandis que `TMG0+0x10` conserve
`0x00600050` pour `HBP=96` et `HSYNC=80`. Le reset vidéo DP a aussi été aligné
sur le pilote constructeur en utilisant le champ complet `VIDEO_RESET[3:0]`.

Cette correction est compilée dans le prochain `bonjour-jupiter2.bin`. Elle
doit encore être exécutée sur la Jupiter 2 et vérifiée visuellement avant de
conclure sur le signal HDMI.

## Lecture du registre DPU Saturn HEE — 18 octobre 2026

La documentation K3 générale décrit les capacités du DPU, mais pas le détail
des registres Saturn HEE. Pour lever cette ambiguïté, le portage a été comparé
au pilote constructeur public [`dpu_saturn_hee.c`](https://github.com/spacemit-com/linux-6.18/blob/k3-br-v1.0.y/drivers/gpu/drm/spacemit/dpu/dpu_saturn_hee.c)
et à ses en-têtes de registres :
[`reg_map_hee.h`](https://github.com/spacemit-com/linux-6.18/blob/k3-br-v1.0.y/drivers/gpu/drm/spacemit/dpu/saturn_regs/reg_map_hee.h),
[`composer_x.h`](https://github.com/spacemit-com/linux-6.18/blob/k3-br-v1.0.y/drivers/gpu/drm/spacemit/dpu/saturn_regs/composer_x.h),
[`dpu_scene_ctl.h`](https://github.com/spacemit-com/linux-6.18/blob/k3-br-v1.0.y/drivers/gpu/drm/spacemit/dpu/saturn_regs/dpu_scene_ctl.h)
et [`cmdlist_top.h`](https://github.com/spacemit-com/linux-6.18/blob/k3-br-v1.0.y/drivers/gpu/drm/spacemit/dpu/saturn_regs/cmdlist_top.h).

La variante active est bien la variante HEE :

| Bloc | Offset DPU1 | Rôle |
|---|---:|---|
| scene control 1 | `0x340` | routage RDMA, sortie, timing, `CFG_READY`, `SW_START` |
| DPU control top | `0x3C0` | activation des canaux cmdlist RDMA/compositeur |
| cmdlist | `0x500` | listes ; compositeur DPU1 sur canal 7 ; validation `CFG_RDY` à `DPU_CTRL_TOP+0x34 = 0x3F4` |
| RDMA0 | `0x1000` | source mémoire et format |
| TBU0 | `0x1100` | traduction DMMU du plane |
| composer CMP1 | `0x30000` | sortie et couches de composition |
| timing TMG0 | `0x51200` | minutage du flux vidéo |

Pour `layer00`, le pilote HEE définit :

```text
CMP1+0x28  layer00_solid_color_r   10 bits
CMP1+0x2C  layer00_solid_color_g   10 bits
CMP1+0x30  layer00_solid_color_b   10 bits
CMP1+0x34  alpha 0:7, solid_area_mode bit 16
CMP1+0x38  en bit 0, blend_sel 1:2, solid_en bit 3, alpha 8:15
```

Le firmware V4 a été corrigé pour programmer les champs de scène aux positions
réelles de `dpu_scene_ctl.h` : `timing_inter0` occupe les bits `6:1` et
`timing_inter1` les bits `12:7`. La valeur attendue de `video_mode` est donc
`0x411` lorsque les deux intervalles valent 8. Le pilote constructeur
`saturn_get_cl_cfg_rdy_addr()` place la dernière ligne de cmdlist à
`DPU1+0x3F4` (`DPU_CTRL_TOP+0x34`). `DPU1+0x534` est réservé dans
`cmdlist_top.h`. Pour le composer 1, les deux bits d'activation de cmdlist
sont à `DPU1+0x3E8` (`DPU_CTRL_TOP+0x28`).
La liste du compositeur DPU1 doit être chargée dans le canal 7
(`rdma_nums=4`, `crtc_id=1`, donc `4+2+1`), comme dans `wb_cmdlist()` du
pilote constructeur.

Cette vérification explique pourquoi les anciens readbacks UART pouvaient être
corrects tout en produisant du noir : ils confirmaient des écritures dans des
registres accessibles, mais pas que les champs HEE étaient encodés aux bons
bits ni que la cmdlist pointait vers son registre `CFG_RDY` réel.

## Validation matérielle du correctif

Le binaire corrigé a été transféré par TFTP puis exécuté depuis U-Boot. La
trace UART confirme le canal 7 du compositeur, `video_mode=0x411`,
`CMP1+0x3E8=0x3`, la couche pleine bleue et le scanout DP :

```text
CMDLIST_STAGE_OK
CMDLIST_ENABLE_OK
DPU_SCENE_STAGE_OK
DPU_LAYER_STAGE_OK
LINK_TRAIN_OK
DP_MSA_STAGE_OK
SCANOUT_STAGE_OK
SATURN_SOLID_BLUE_HOLD
```

Le chemin DPU/DP est actif sur la carte ; l'observation du moniteur doit
maintenant confirmer le bleu solide produit par Saturn.

## Échec d'affichage du flux Saturn — 18 octobre 2026

Le test qui a suivi a désactivé la mire DP et a demandé à `CMP1` de produire
directement un fond rouge, avec `layer00` désactivée :

```text
DP_COLORBAR_TEST=false
TMG_BACKGROUND_TEST=false
CMP_BACKGROUND_TEST=true
SATURN_SOLID_TEST=true
CMP1+0x08_BG_R_RB=0x000003FF
CMP1+0x0C_BG_G_RB=0x00000000
CMP1+0x10_BG_B_RB=0x00000000
CMP1+0x38_ACTIVE_RB=0x00000000
CMP_BACKGROUND_RED_HOLD
```

L'écran est resté noir. Cette observation invalide la conclusion précédente
selon laquelle les readbacks `CMP1`, `CMDLIST_ENABLE_OK` et
`SCANOUT_STAGE_OK` suffiraient à prouver la production de pixels. Ils prouvent
que les registres sont accessibles et que la séquence de contrôle se termine ;
ils ne prouvent pas que le flux Saturn atteint effectivement le TMG puis le
transmetteur DP.

Les tests de référence ont pourtant réussi : Bianbu affiche l'écran de
connexion, le fond rouge programmé directement par TMG est visible et la mire
DP interne bleue est visible avec la même chaîne USB-C/HDMI/DVI. Le défaut
reste donc dans le chemin DPU1/Saturn vers le scanout normal, probablement dans
le commit effectif de scène/compositeur, le routage de cmdlist, le plane
RDMA/DMMU ou la sélection de la source TMG.

État de validation : `SPLASH_OK` n'est pas obtenu, le splash BGRA n'est pas
affiché et le bring-up vidéo Saturn est suspendu sur cet échec reproductible.
