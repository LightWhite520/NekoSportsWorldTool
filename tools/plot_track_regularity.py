"""Plot the synthetic output of track::regularity_tests::offline_comparison.

Requires matplotlib. Export the fixture with NEKO_TRACK_COMPARISON_DIR, then:
python tools/plot_track_regularity.py target/track-drift/after docs/track-regularity.png
Only the generated road.json and track.json fixture files are read.
"""
import argparse
import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    traces = [json.loads((args.input / name).read_text(encoding="utf-8"))
              for name in ("road.json", "track.json")]
    # Match tests/common/stadium.rs; axes show only relative metres.
    scale_x = 111320 * np.cos(np.deg2rad(30))
    paths = [np.array([[(p["gLng"] - 110) * scale_x,
                       (p["gLat"] - 30) * 111132] for p in t["locations"]])
             for t in traces]
    outline = []
    for cy, start in ((40, 0), (-40, np.pi)):
        a = np.linspace(start, start + np.pi, 25)
        outline.extend(zip(36 * np.cos(a), cy + 36 * np.sin(a)))
    outline = np.array(outline + [outline[0]])

    plt.rcParams.update({"font.family": "DejaVu Sans", "font.size": 11})
    fig, axes = plt.subplots(2, 2, figsize=(11, 10.5),
                             gridspec_kw={"height_ratios": [2.1, 1]})
    colors = ["#e27542", "#16846d"]
    titles = ["Existing road mode (unchanged)", "Track mode (regular)"]
    metrics = []
    for column, (trace, xy, color, title) in enumerate(zip(traces, paths, colors, titles)):
        times = np.array([p["totalTime"] for p in trace["locations"]])
        intervals = np.diff(times)
        positive = intervals[intervals > 0]
        max_step = np.linalg.norm(np.diff(xy, axis=0), axis=1).max()
        metrics.append(f'{len(xy)} points | median interval {np.median(positive):.0f} s'
                       f' | max adjacent displacement {max_step:.1f} m')
        for row in range(2):
            ax = axes[row, column]
            ax.plot(outline[:, 0], outline[:, 1], color="#bac4cc", lw=4,
                    label="Planned route", zorder=1)
            ax.plot(xy[:, 0], xy[:, 1], color=color, lw=0.9, alpha=0.75,
                    label="Generated samples", zorder=2)
            ax.scatter(xy[:, 0], xy[:, 1], color=color, s=3, alpha=0.6, zorder=3)
            ax.set_aspect("equal", adjustable="box")
            ax.grid(alpha=0.2)
            ax.set_xlabel("East / m")
            ax.set_ylabel("North / m")
            ax.spines[["top", "right"]].set_visible(False)
        axes[0, column].set(xlim=(-60, 60), ylim=(-95, 95), title=title)
        axes[0, column].legend(loc="upper right", fontsize=9)
        axes[1, column].set(xlim=(22, 42), ylim=(35, 65), title="Same bend, enlarged")
    fig.suptitle("Running track: route drift comparison", fontsize=20, weight="bold", y=0.97)
    fig.text(0.5, 0.928, "Synthetic fixture | 2,110 m / 15 min | seed 7 | identical road network",
             ha="center", color="#52606d")
    fig.text(0.07, 0.085, "Road: " + metrics[0], fontsize=10, color=colors[0])
    fig.text(0.07, 0.063, "Track: " + metrics[1], fontsize=10, color=colors[1])
    fig.text(0.07, 0.031,
             "Track offset bound: 0.35 m. Offline geometry check only; no platform acceptance test.",
             fontsize=10, color="#52606d")
    fig.subplots_adjust(left=0.08, right=0.96, top=0.88, bottom=0.15, wspace=0.3, hspace=0.38)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    fig.savefig(args.output, dpi=160, facecolor="white")
    print(args.output)
    for title, metric in zip(titles, metrics):
        print(f"{title}: {metric}")


if __name__ == "__main__":
    main()
