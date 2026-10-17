# Assets V3 repris par V4

Ressources de test non exécutables reprises du jeu d'assets V3 : images
(PNG/JPEG/TIFF), polices, icônes et shaders. Les fichiers FLAC sont
volontairement reportés jusqu'à la validation du domaine `audio.devices`.

Ce répertoire est destiné aux outils hôte, à l'UI et au futur chemin
egui/Vulkan de TacticalGlobe. Il n'est pas une image firmware et ne doit pas
être copié dans `tftp-root/` pour le boot de la Jupiter 2.

Les licences et la provenance des polices, images et shaders sont suivies dans
[`../THIRD_PARTY_NOTICES_FR.md`](../THIRD_PARTY_NOTICES_FR.md) et sa version
anglaise [`../THIRD_PARTY_NOTICES_EN.md`](../THIRD_PARTY_NOTICES_EN.md). Toute nouvelle
ressource doit y être ajoutée avant une release publique.

## État

Les assets servent aux tests de chargement, de métriques de texte et de rendu.
Ils ne sont pas chargés par le chemin minimal `UILab` actuellement validé dans
QEMU : ce profil affiche le chrome de fenêtre et son curseur. Les assets
utilisés par une nouvelle scène doivent rester bornés, versionnés et validés
avant d’être ajoutés à l’image MFFS.

`images/background.jpg` est la source du fond du Screen. Le fichier
`images/background-1920x1080.bgra` est sa version recadrée et convertie pour le
backend GPU RISC-V ; il conserve le format BGRA attendu par le scanout QEMU.
`intuition.library` publie l'identifiant du fond, `graphics.library` le place
avant les primitives de fenêtre, puis l'adaptateur Venus résout cet identifiant
et charge l'image dans la mémoire du scanout avant de composer UILab. Jupiter 2
pourra conserver le même identifiant et fournir sa propre résolution d'asset.

Les fichiers `.spv` sont des modules SPIR-V déjà compilés. Ils pourront servir
à la validation Vulkan côté hôte après confirmation du pilote RGX ; aucun
chargement matériel RGX n'est encore déclaré opérationnel sur Jupiter 2.

## Mise à jour au 5 septembre 2026

Les fontes `FreeSans.ttf` et `FreeSansBold.ttf` servent au chemin UTF-8 direct
du titre UILab. Les masques TTF restent bornés et sont consommés par le moteur
de fontes partagé ; le backend GPU décide ensuite de leur composition.

## État V4 — 17 octobre 2026

L'inventaire des assets est présent dans le dépôt. Les ressources ne sont pas
encore incluses dans une image MFFS ni reliées à une scène V4 ; cette étape
attend la validation de `firmware/bonjour`, du transport de commandes et du
backend graphique.
