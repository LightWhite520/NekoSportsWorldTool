"""Plot two coordinate-free motion-check --timeline JSON files.

Requires matplotlib and numpy. Use only synthetic records in public reports.
"""
import argparse
import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib import font_manager
import numpy as np


def load(path):
    report = json.loads(path.read_text(encoding="utf-8-sig"))
    timeline = report["timeline"]
    time = np.array([p["time_s"] for p in timeline], dtype=float)
    speed = np.array([p["speed_m_s"] for p in timeline], dtype=float)
    steps = np.array([p["steps"] for p in timeline], dtype=float)
    if len(time) < 4 or not np.all(np.diff(time) == 1):
        raise ValueError("Expected a one-second timeline with at least four samples")
    if not np.all(np.isfinite(speed)) or not np.all(np.isfinite(steps)):
        raise ValueError("Non-finite samples")
    return report, time, speed, steps


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    inputs = [load(args.before), load(args.after)]
    before, after = inputs[0][0], inputs[1][0]
    if before["model"] != "motion-v2" or after["model"] != "motion-v3":
        raise ValueError("This figure describes the development v2-to-v3 comparison")
    for field in ("seed", "duration_s"):
        if before[field] != after[field]:
            raise ValueError(f"Mismatched {field}")
    target = after["distance_m"] - after["distance_error_m"]
    if abs(before["distance_m"] - before["distance_error_m"] - target) > 1e-6:
        raise ValueError("Mismatched target distance")
    if not np.array_equal(inputs[0][1], inputs[1][1]):
        raise ValueError("Mismatched sample times")
    duration = after["duration_s"]
    trim = min(30, duration // 10)
    zoom_start, zoom_end = duration * .15, duration * .30
    available_fonts = {font.name for font in font_manager.fontManager.ttflist}
    font = next((name for name in ("Microsoft YaHei", "Noto Sans CJK SC", "SimHei")
                 if name in available_fonts), None)
    if font is None:
        raise ValueError("Install Microsoft YaHei, Noto Sans CJK SC or SimHei to render Chinese labels")
    plt.rcParams.update({
        "font.family": font,
        "axes.unicode_minus": False, "font.size": 11,
        "axes.spines.top": False, "axes.spines.right": False,
        "svg.fonttype": "none", "svg.hashsalt": "motion-v2-v3",
    })
    fig = plt.figure(figsize=(15, 12.8), facecolor="#f5f8fb", layout="constrained")
    grid = fig.add_gridspec(4, 2, height_ratios=[1, 1, .95, .43])
    axes = [fig.add_subplot(grid[row, col]) for row, col in ((0, 0), (0, 1), (1, 0), (1, 1))]
    table_ax = fig.add_subplot(grid[2, :])
    note_ax = fig.add_subplot(grid[3, :])
    table_ax.axis("off")
    note_ax.axis("off")
    labels = ("开发中间版 v2", "当前版本 v3")
    colors = ("#c7793d", "#00838a")
    metrics = []
    for (report, time, speed, steps), label, color in zip(inputs, labels, colors):
        cadence = np.diff(steps) * 60
        accel = np.diff(speed)
        jerk = np.diff(speed, n=2)
        for ax, x, values in zip(axes, (time, time[1:], time[1:], time[2:]),
                                (speed, cadence, accel, jerk)):
            zoom = (x >= zoom_start) & (x <= zoom_end)
            ax.plot(x[zoom], values[zoom], label=label, color=color, lw=1.65, alpha=.9)
        interior = (time >= trim) & (time <= duration - trim)
        v = speed[interior]
        c = np.diff(steps[interior]) * 60
        rms = lambda values: float(np.sqrt(np.mean(values * values)))
        metrics.append({
            "model": report["model"],
            "speed_std_m_s": float(np.std(v)),
            "acceleration_rms_m_s2": rms(np.diff(v)),
            "jerk_rms_m_s3": rms(np.diff(v, n=2)),
            "cadence_delta_rms_spm": rms(np.diff(c)),
            "distance_error_m": report["distance_error_m"],
            "total_steps_unrounded": float(steps[-1]),
        })
    titles = ("速度 / 配速：保留缓慢起伏", "步频：随速度变化并平滑扰动",
              "加速度：相邻秒速度差", "加速度变化率：相邻秒加速度差")
    units = ("速度（m/s）", "步频（步/分）", "加速度（m/s²）", "加速度变化率（m/s³）")
    for ax, title, unit in zip(axes, titles, units):
        ax.set_title(title, loc="left", fontsize=13, pad=12)
        ax.set_xlabel("时间（秒）")
        ax.set_ylabel(unit)
        ax.grid(alpha=.16)
        ax.set_facecolor("white")
    axes[0].legend(loc="upper left", frameon=False, ncol=2, fontsize=10)
    reciprocal = lambda values: 1000 / (60 * np.maximum(values, 1e-9))
    pace_axis = axes[0].secondary_yaxis("right", functions=(reciprocal, reciprocal))
    pace_axis.set_ylabel("配速（分钟/公里）")
    specs = (
        ("速度标准差（m/s）", "speed_std_m_s", 4, True),
        ("加速度 RMS（m/s²）", "acceleration_rms_m_s2", 4, False),
        ("加速度变化率 RMS（m/s³）", "jerk_rms_m_s3", 4, False),
        ("步频逐秒变化 RMS（步/分）", "cadence_delta_rms_spm", 3, False),
    )
    rows, changes = [], {}
    for label, key, digits, retain in specs:
        old, new = metrics[0][key], metrics[1][key]
        ratio = new / old
        changes[key] = {"retained_percent": ratio * 100, "reduced_percent": (1-ratio) * 100}
        change = f"保留 {ratio:.1%}" if retain else f"下降 {1-ratio:.1%}"
        rows.append([label, f"{old:.{digits}f}", f"{new:.{digits}f}", change])
    table_ax.set_title(f"统计范围：第 {trim}–{duration-trim} 秒，排除起停阶段", loc="left", fontsize=13, pad=10)
    table = table_ax.table(cellText=rows, colLabels=["指标", *labels, "变化"],
                          colWidths=[.43, .19, .19, .19], bbox=[0, .03, 1, .9], cellLoc="center")
    table.auto_set_font_size(False)
    table.set_fontsize(12)
    for (row, col), cell in table.get_celld().items():
        cell.set_edgecolor("#d8e2eb")
        cell.set_facecolor("#e5eef3" if row == 0 else "white")
        if row == 0:
            cell.set_text_props(weight="bold")
        if row > 0 and col == 3:
            cell.set_text_props(color="#00787e", weight="bold")
    notes = (
        "参数：配速扰动相关时间 24 秒 / 响应时间 4 秒；步频扰动 10 秒 / 3 秒（工程默认值）。\n"
        "步频由未取整累计步数的差分计算；协议整型步数按单秒反推会有量化跳动。RMS = 均方根。\n"
        "基线是本次开发中的 v2，不是上游 main；仅使用虚构路线，尚未由真人数据校准。\n"
        "以上统计不覆盖起停与急弯；当前不保证全程加速度连续或统一的加速度变化率上限。"
    )
    note_ax.text(0, .95, notes, transform=note_ax.transAxes, va="top", fontsize=11,
                 color="#45556b", linespacing=1.7)
    fig.suptitle(f"速度与步频平滑性 · 离线对比\n虚构圆环 / {target:,.0f} 米 / {duration} 秒 / seed = {after['seed']}", fontsize=19)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    fig.savefig(args.output_dir / "motion-comparison.png", dpi=160, bbox_inches="tight")
    svg_path = args.output_dir / "motion-comparison.svg"
    fig.savefig(svg_path, bbox_inches="tight", metadata={"Date": None})
    svg_path.write_text("\n".join(line.rstrip() for line in svg_path.read_text(encoding="utf-8").splitlines()) + "\n",
                        encoding="utf-8")
    summary = {
        "baseline": "Development intermediate motion-v2, not upstream main",
        "fixture": "synthetic circle", "distance_m": target, "duration_s": duration,
        "seed": after["seed"], "statistics_interval_s": [trim, duration-trim],
        "plot_interval_s": [zoom_start, zoom_end],
        "cadence_source": "Differences of unrounded cumulative steps, multiplied by 60",
        "metrics": metrics, "changes": changes,
    }
    (args.output_dir / "comparison-metrics.json").write_text(
        json.dumps(summary, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps(summary, indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
