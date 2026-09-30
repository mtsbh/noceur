# Key model

`openkeyscan2.bin` holds the weights `noceur-audio::key` runs: KeyNet (Korzeniowski & Widmer,
"Genre-Agnostic Key Classification With Convolutional Neural Networks", ISMIR 2018), as retrained
by OpenKeyScan on a superset of the GiantSteps-MTG key dataset (Beatport electronic music).

- Source: `checkpoints/openkeyscan2.pt` from
  [rekordcloud/openkeyscan-analyzer](https://github.com/rekordcloud/openkeyscan-analyzer)
  (commit `d5cac64`, SHA-256 `4f73d508…cbcde206cbe7`), a fork of
  [a1ex90/MusicalKeyCNN](https://github.com/a1ex90/MusicalKeyCNN). MIT licence,
  © 2025 Alexander Sommer (text below).
- Converted with `export.py` in this folder: the checkpoint is loaded with
  `torch.load(weights_only=True)`, each batch norm is folded into the convolution before it, and the
  result is written as a flat little-endian file (`NKEY`, version 1, nine layers of
  `out, in, kernel, weights, bias`).

```sh
python export.py path/to/openkeyscan2.pt openkeyscan2.bin
```

## Measured

On the 604 tracks of the GiantSteps key test set (MIREX weighted score / exactly correct):

| | weighted | correct |
|---|---|---|
| this model, in Rust (`tests/key_giantsteps.rs`) | 76.46 | 71.52 |
| MusicalKeyCNN's own `keynet.pt`, the same network | 73.82 | 67.22 |
| OpenKeyScan's `openkeyscan3.pt` (10× wider) | 76.87 | 71.03 |
| Mixed In Key 10.3 (answers published in MusicalKeyCNN) | 75.70 | 69.37 |
| rekordbox 7.12 (same) | 65.53 | 56.79 |

The Rust port gives the same key as PyTorch on 603 of the 604 tracks; on the other the model is
undecided (its top class has 8.8%). Its constant-Q spectrogram is within 0.2–0.4% of librosa's.
OpenKeyScan has not published its training data, so it cannot be ruled out that it saw these
tracks.

Against 300 tracks of a DJ library tagged by Mixed In Key (`tests/key_library.rs`, `NOCEUR_MIK=1`),
none of them in any training set:

| | weighted | same key as MIK |
|---|---|---|
| `openkeyscan2` | 72.6 | 65.0% |
| `openkeyscan3` | 69.0 | 61.7% |
| `keynet` | 68.0 | 61.0% |

`openkeyscan3` is also 2.3× slower and 10× larger, so the smaller model ships.

## Licence

```
MIT License

Copyright (c) 2025 Alexander Sommer

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```
