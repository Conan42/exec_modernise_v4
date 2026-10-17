# Third-party component notices

**Languages:** [Français](THIRD_PARTY_NOTICES_FR.md) · **English**

The Rust workspace code is licensed under MIT; see [`LICENSE`](LICENSE).
Resources grouped under `assets/` may have different licences.

## Fonts

| Files | Source | Licence |
| --- | --- | --- |
| `FreeSans*.ttf` | GNU FreeFont | GNU GPL v3 or later, with the font distribution conditions; [GNU FreeFont project](https://savannah.gnu.org/projects/freefont/) |
| `MaterialIcons-Regular.ttf` | Google Material Icons | Apache License 2.0; [Material Icons repository](https://github.com/google/material-design-icons) |
| `fa-solid-900.ttf` | Font Awesome 6 Free | Font under SIL OFL 1.1; icons have the Font Awesome terms; [licence](https://fontawesome.com/license/free) |
| `materialdesignicons-webfont.ttf` | Material Design Icons | Apache License 2.0; [MDI repository](https://github.com/Templarian/MaterialDesign) |

The complete licence files from third-party projects must be kept with any
redistribution that uses them. The exact provenance of each binary copy
imported from V3 must be checked before a public release.

## Images and shaders

The BGRA/JPEG images and shaders in `assets/` come from V3 work or V4
development. Their copyright and redistribution status must be confirmed
individually before publishing a release or binary package. The bring-up
firmware only needs the test BGRA framebuffer; shaders are not loaded by
Jupiter 2 at this stage.

## SpacemiT / K3 documentation

Hardware documents imported under `docs/jupiter2/` remain manufacturer
reference documents and are kept for development. They are not original work
of the V4 project. Check their redistribution terms with the source before
creating a release that includes them.
