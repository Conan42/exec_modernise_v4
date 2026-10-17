# Préparer la publication GitHub

Le dépôt est prêt pour une première revue publique locale. La publication
elle-même demande encore de choisir le compte ou l'organisation GitHub, le nom
du dépôt et la licence des assets dont la provenance vient de V3.

## État de préparation

- licence MIT ajoutée pour le code Rust ;
- métadonnées de licence ajoutées à toutes les crates ;
- CI GitHub ajoutée dans `.github/workflows/ci.yml` ;
- architecture et flux documentés en Mermaid ;
- règles de contribution et notices d'assets ajoutées ;
- `tftp-root/` exclu des commits, car il contient des artefacts générés ;
- aucun secret, mot de passe ou token trouvé dans les fichiers publics.

## Avant le premier push

1. Vérifier les droits de redistribution des images, shaders et documents
   matériels décrits dans [`../THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md).
2. Relire les traces de [`../JUPITER2_BRINGUP.md`](../JUPITER2_BRINGUP.md) et
   retirer les paramètres de laboratoire qui ne doivent pas être publics.
3. Choisir le nom du dépôt et créer un dépôt GitHub vide, sans README généré.
4. Ajouter l'URL distante puis pousser la branche principale :

   ```bash
   git remote add origin <URL_DU_DEPOT_GITHUB>
   git push -u origin main
   ```

5. Activer les issues et les discussions après la première lecture de la CI.
6. Ajouter les topics `rust`, `risc-v`, `jupiter-2`, `embedded` et
   `displayport` si le nom du dépôt est confirmé.

## Première release

La première release doit rester une release de développement. Elle peut
contenir le code Rust, les scripts, la documentation et les artefacts générés
reproductibles. Elle ne doit pas présenter le splash comme fonctionnel tant que
le HP Compaq LA2006x n'a pas affiché le signal et que `SPLASH_OK` n'est pas
observé.
