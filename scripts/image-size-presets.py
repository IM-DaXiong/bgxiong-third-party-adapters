"""Compile declared pixel budgets and width:height ratios into an adapter-owned table."""
import argparse
import json
from math import gcd, isqrt
from pathlib import Path


def dimensions(pixels, ratio, alignment):
    width, height = map(int, ratio.split(":"))
    if min(width, height, alignment, pixels) <= 0:
        raise ValueError("Image dimensions and alignment must be positive")
    divisor = gcd(width, height)
    width, height = width // divisor, height // divisor
    unit = width * height * alignment ** 2
    lower = max(1, isqrt(pixels // unit))
    scale = min((lower, lower + 1), key=lambda n: abs(n * n * unit - pixels)) * alignment
    return f"{width * scale}x{height * scale}"


def compile_sizes(spec):
    result = {}
    for tier, pixels in spec["pixels"].items():
        result[tier] = {}
        for ratio in spec["ratios"]:
            size = spec.get("presets", {}).get(tier, {}).get(ratio) or dimensions(pixels, ratio, spec["alignment"])
            w, h = map(int, size.split("x"))
            if not spec["minPixels"] <= w * h <= spec["maxPixels"]:
                raise ValueError(f"{tier} {ratio} exceeds pixel limits")
            result[tier][ratio] = size
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("spec", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    table = compile_sizes(json.loads(args.spec.read_bytes()))
    data = (json.dumps(table, indent=2) + "\n").encode()
    if args.check:
        if args.output.read_bytes() != data:
            parser.exit(1, "Image size table is stale\n")
    else:
        args.output.write_bytes(data)
