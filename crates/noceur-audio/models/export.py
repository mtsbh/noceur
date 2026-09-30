"""Writes a KeyNet checkpoint as the flat file noceur-audio embeds.

Batch norm is folded into each convolution (inference only), so the Rust side
is nine plain convolutions with a bias, each followed by ELU.

Layout, little-endian: b"NKEY", u32 version=1, u32 layer count, then per
layer u32 out, u32 in, u32 kernel, f32 weights[out*in*k*k], f32 bias[out]."""
import struct, sys, torch, numpy as np
ckpt, out = sys.argv[1], sys.argv[2]
sd = torch.load(ckpt, map_location="cpu", weights_only=True)
layers = []
for i in range(1, 10):
    w = sd[f"conv{i}.conv.weight"].double().numpy()
    g = sd[f"conv{i}.bn.weight"].double().numpy()
    b = sd[f"conv{i}.bn.bias"].double().numpy()
    m = sd[f"conv{i}.bn.running_mean"].double().numpy()
    v = sd[f"conv{i}.bn.running_var"].double().numpy()
    scale = g / np.sqrt(v + 1e-5)
    layers.append((w * scale[:, None, None, None], b - m * scale))
with open(out, "wb") as f:
    f.write(b"NKEY" + struct.pack("<II", 1, len(layers)))
    for w, b in layers:
        co, ci, k, _ = w.shape
        f.write(struct.pack("<III", co, ci, k))
        f.write(w.astype("<f4").tobytes()); f.write(b.astype("<f4").tobytes())
print(out, [l[0].shape for l in layers])
