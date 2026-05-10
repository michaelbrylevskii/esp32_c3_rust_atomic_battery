#!/usr/bin/env python3
"""Generate the documentation wiring diagram.

The diagram is intentionally drawn as a readable wiring map, not as an EDA
schematic. Coordinates are explicit so labels, pins, and wires stay stable in
Markdown renderers and SVG viewers.
"""

from __future__ import annotations

from html import escape
from pathlib import Path


ROOT = Path(__file__).resolve().parent
OUT = ROOT / "wiring.svg"

WIDTH = 1460
HEIGHT = 930

POWER = "#c2410c"
GROUND = "#334155"
I2C = "#0f766e"
DISPLAY = "#0e7490"
LED = "#7c3aed"
SWITCH = "#2563eb"
BLACK = "#111827"
MUTED = "#4b5563"
FILL = "#f8fafc"
BG = "#ffffff"

parts: list[str] = []


def add(value: str) -> None:
    parts.append(value)


def label(
    value: str,
    x: int,
    y: int,
    *,
    size: int = 18,
    color: str = BLACK,
    anchor: str = "middle",
    weight: str = "400",
) -> None:
    for index, line in enumerate(value.split("\n")):
        add(
            f'<text x="{x}" y="{y + index * int(size * 1.28)}" '
            f'text-anchor="{anchor}" font-family="Arial, sans-serif" '
            f'font-size="{size}" font-weight="{weight}" fill="{color}">'
            f"{escape(line)}</text>"
        )


def line(
    x1: int,
    y1: int,
    x2: int,
    y2: int,
    *,
    color: str,
    width: int = 3,
) -> None:
    add(
        f'<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" '
        f'stroke="{color}" stroke-width="{width}" stroke-linecap="round" />'
    )


def wire(points: list[tuple[int, int]], *, color: str, width: int = 3) -> None:
    for (x1, y1), (x2, y2) in zip(points, points[1:]):
        line(x1, y1, x2, y2, color=color, width=width)


def vertical_wire_with_jumps(
    x: int,
    y1: int,
    y2: int,
    jumps: list[int],
    *,
    color: str,
    width: int = 3,
) -> None:
    """Draw a vertical wire with small jump arcs at non-connected crossings."""
    current = y1
    for jump_y in sorted(jump for jump in jumps if y1 < jump < y2):
        line(x, current, x, jump_y - 8, color=color, width=width)
        add(
            f'<path d="M {x} {jump_y - 8} '
            f'Q {x + 14} {jump_y} {x} {jump_y + 8}" '
            f'fill="none" stroke="{color}" stroke-width="{width}" stroke-linecap="round" />'
        )
        current = jump_y + 8
    line(x, current, x, y2, color=color, width=width)


def box(x: int, y: int, w: int, h: int, title: str, subtitle: str | None = None) -> None:
    add(
        f'<rect x="{x}" y="{y}" width="{w}" height="{h}" rx="12" '
        f'fill="{FILL}" stroke="{BLACK}" stroke-width="3" />'
    )
    label(title, x + w // 2, y + 42, size=25, weight="500")
    if subtitle:
        label(subtitle, x + w // 2, y + 76, size=15, color=MUTED)


def dot(x: int, y: int, *, color: str) -> None:
    add(f'<circle cx="{x}" cy="{y}" r="5" fill="{color}" />')


def terminal(x: int, y: int, *, color: str = BLACK) -> None:
    add(f'<circle cx="{x}" cy="{y}" r="4" fill="{color}" />')


def power_input_stub(x: int, y: int, *, color: str, label_text: str, label_y_offset: int) -> None:
    """Draw an explicit external power input stub for a rail."""
    line(x - 26, y, x, y, color=color, width=4)
    terminal(x - 26, y, color=color)
    label(label_text, x - 34, y + label_y_offset, size=14, color=color, anchor="end")


def resistor_vertical(x: int, y1: int, y2: int) -> None:
    """Draw one vertical pull-up resistor from the power rail to a signal lane."""
    line(x, y1, x, y1 + 18, color=POWER, width=3)
    points = [
        (x, y1 + 18),
        (x - 10, y1 + 31),
        (x + 10, y1 + 44),
        (x - 10, y1 + 57),
        (x + 10, y1 + 70),
        (x - 10, y1 + 83),
        (x, y1 + 96),
    ]
    add(
        '<polyline points="'
        + " ".join(f"{x0},{y0}" for x0, y0 in points)
        + f'" fill="none" stroke="{BLACK}" stroke-width="3" '
        + 'stroke-linecap="round" stroke-linejoin="round" />'
    )
    line(x, y1 + 96, x, y2, color=I2C, width=3)


def resistor_horizontal(x1: int, x2: int, y: int, *, color: str, right_color: str | None = None) -> None:
    """Draw one horizontal series resistor on a signal wire."""
    right_color = right_color or color
    lead = 14
    line(x1, y, x1 + lead, y, color=color, width=3)
    line(x2 - lead, y, x2, y, color=right_color, width=3)
    points = [
        (x1 + lead, y),
        (x1 + lead + 12, y - 10),
        (x1 + lead + 24, y + 10),
        (x1 + lead + 36, y - 10),
        (x1 + lead + 48, y + 10),
        (x1 + lead + 60, y - 10),
        (x2 - lead, y),
    ]
    add(
        '<polyline points="'
        + " ".join(f"{x0},{y0}" for x0, y0 in points)
        + f'" fill="none" stroke="{BLACK}" stroke-width="3" '
        + 'stroke-linecap="round" stroke-linejoin="round" />'
    )


def led_symbol(x: int, y: int, *, color: str) -> None:
    """Draw an active-high LED from left anode to right cathode."""
    triangle = [(x, y - 20), (x, y + 20), (x + 38, y)]
    add(
        '<polygon points="'
        + " ".join(f"{x0},{y0}" for x0, y0 in triangle)
        + f'" fill="none" stroke="{BLACK}" stroke-width="3" stroke-linejoin="round" />'
    )
    line(x + 44, y - 23, x + 44, y + 23, color=BLACK, width=3)
    line(x + 56, y - 23, x + 66, y - 33, color=color, width=2)
    line(x + 64, y - 14, x + 74, y - 24, color=color, width=2)
    line(x + 38, y, x + 44, y, color=color, width=3)


def latching_control_to_ground(x: int, y: int, ground_y: int) -> None:
    """Draw a latching switch/button from the signal node to GND."""
    terminal(x, y, color=SWITCH)
    line(x, y, x + 38, y, color=SWITCH, width=3)
    line(x + 62, y - 20, x + 42, y, color=BLACK, width=3)
    line(x + 84, y, x + 100, y, color=GROUND, width=3)
    line(x + 100, y, x + 100, ground_y, color=GROUND, width=3)
    label("toggle switch / latching button", x + 50, y + 34, size=14, color=MUTED)
    label("to GND", x + 116, y + 5, size=13, color=GROUND, anchor="start")


def pin_text(value: str, x: int, y: int, *, side: str, color: str = BLACK) -> None:
    """Place a pin label inside the component, away from the wire lane."""
    if side == "left":
        label(value, x + 18, y + 5, size=14, anchor="start", color=color)
    elif side == "right":
        label(value, x - 18, y + 5, size=14, anchor="end", color=color)
    else:
        raise ValueError(f"unsupported side: {side}")


def start_svg() -> None:
    add(
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" '
        f'viewBox="0 0 {WIDTH} {HEIGHT}" role="img" aria-labelledby="title desc">'
    )
    add('<title id="title">ESP32-C3 Super Mini wiring diagram</title>')
    add(
        '<desc id="desc">Wiring diagram for ESP32-C3 Super Mini, PN532, '
        "TM1637, status LEDs, and activation switch.</desc>"
    )
    add(f'<rect x="0" y="0" width="{WIDTH}" height="{HEIGHT}" fill="{BG}" />')


def main() -> None:
    start_svg()

    label("ESP32-C3 Super Mini wiring", WIDTH // 2, 42, size=27, weight="600")
    label(
        "PN532 I2C, TM1637 display, active-high LEDs, switch to GND",
        WIDTH // 2,
        74,
        size=15,
        color=MUTED,
    )

    power_y = 125
    ground_y = 860
    rail_x1 = 110
    power_bus_x = 1325
    ground_bus_x = 1360

    # Shared rails. All modules connect to these rails through short stubs.
    wire([(rail_x1, power_y), (power_bus_x, power_y)], color=POWER, width=4)
    power_input_stub(rail_x1, power_y, color=POWER, label_text="power in", label_y_offset=-12)
    label("3.3V", rail_x1 + 8, power_y - 16, size=18, color=POWER, anchor="start", weight="600")
    wire([(rail_x1, ground_y), (ground_bus_x, ground_y)], color=GROUND, width=4)
    power_input_stub(rail_x1, ground_y, color=GROUND, label_text="power in", label_y_offset=18)
    label("GND", rail_x1 + 8, ground_y + 32, size=18, color=GROUND, anchor="start", weight="600")

    # Component blocks.
    esp_x, esp_y, esp_w, esp_h = 135, 190, 325, 630
    right_x, right_w = 980, 315
    box(esp_x, esp_y, esp_w, esp_h, "ESP32-C3", "Super Mini")
    box(right_x, 185, right_w, 150, "PN532", "NFC reader, I2C mode")
    box(right_x, 385, right_w, 150, "TM1637", "4-digit display + colon")

    # Pin coordinates. Signal lanes are horizontal and do not cross.
    esp_right = esp_x + esp_w
    esp_left = esp_x
    esp = {
        "3V3": (esp_left, 255),
        "GND": (esp_left, 650),
        "SDA": (esp_right, 250),
        "SCL": (esp_right, 300),
        "CLK": (esp_right, 430),
        "DIO": (esp_right, 480),
        "RED": (esp_right, 620),
        "GREEN": (esp_right, 680),
        "SWITCH": (esp_right, 785),
    }
    pn = {
        "SDA": (right_x, 250),
        "SCL": (right_x, 300),
        "VCC": (right_x + right_w, 240),
        "GND": (right_x + right_w, 300),
        "RST": (right_x + right_w // 2, 335),
    }
    tm = {
        "CLK": (right_x, 430),
        "DIO": (right_x, 480),
        "VCC": (right_x + right_w, 430),
        "GND": (right_x + right_w, 490),
    }
    button = (940, 785)

    # Pin labels live inside components so wires never run through text.
    for name, point in [
        ("3V3", esp["3V3"]),
        ("GND", esp["GND"]),
    ]:
        pin_text(name, *point, side="left")

    for name, point in [
        ("GPIO3 / SDA", esp["SDA"]),
        ("GPIO4 / SCL", esp["SCL"]),
        ("GPIO5 / CLK", esp["CLK"]),
        ("GPIO6 / DIO", esp["DIO"]),
        ("GPIO0 / RED", esp["RED"]),
        ("GPIO1 / GREEN", esp["GREEN"]),
    ]:
        pin_text(name, *point, side="right")
    label("GPIO10 / SWITCH", esp["SWITCH"][0] - 18, esp["SWITCH"][1] - 10, size=14, anchor="end")

    for name, point in [("SDA", pn["SDA"]), ("SCL", pn["SCL"])]:
        pin_text(name, *point, side="left")
    for name, point in [("VCC", pn["VCC"]), ("GND", pn["GND"])]:
        pin_text(name, *point, side="right")

    for name, point in [("CLK", tm["CLK"]), ("DIO", tm["DIO"])]:
        pin_text(name, *point, side="left")
    for name, point in [("VCC", tm["VCC"]), ("GND", tm["GND"])]:
        pin_text(name, *point, side="right")

    # Terminals make it explicit where wires attach to a block.
    for point, color in [
        (esp["3V3"], POWER),
        (esp["GND"], GROUND),
        (esp["SDA"], I2C),
        (esp["SCL"], I2C),
        (esp["CLK"], DISPLAY),
        (esp["DIO"], DISPLAY),
        (esp["RED"], LED),
        (esp["GREEN"], LED),
        (esp["SWITCH"], SWITCH),
        (pn["SDA"], I2C),
        (pn["SCL"], I2C),
        (pn["VCC"], POWER),
        (pn["GND"], GROUND),
        (tm["CLK"], DISPLAY),
        (tm["DIO"], DISPLAY),
        (tm["VCC"], POWER),
        (tm["GND"], GROUND),
    ]:
        terminal(*point, color=color)

    # Power and ground connections.
    wire([esp["3V3"], (rail_x1, esp["3V3"][1]), (rail_x1, power_y)], color=POWER)
    wire([esp["GND"], (rail_x1, esp["GND"][1]), (rail_x1, ground_y)], color=GROUND)
    dot(rail_x1, power_y, color=POWER)
    dot(rail_x1, ground_y, color=GROUND)

    vertical_wire_with_jumps(
        power_bus_x,
        power_y,
        tm["VCC"][1],
        jumps=[pn["GND"][1]],
        color=POWER,
        width=4,
    )
    wire([(ground_bus_x, ground_y), (ground_bus_x, pn["GND"][1])], color=GROUND, width=4)
    for point in [pn["VCC"]]:
        wire([point, (power_bus_x, point[1])], color=POWER)
        dot(power_bus_x, point[1], color=POWER)
    wire([tm["VCC"], (power_bus_x, tm["VCC"][1])], color=POWER)
    wire([pn["GND"], (ground_bus_x, pn["GND"][1])], color=GROUND)
    for point in [tm["GND"]]:
        wire([point, (ground_bus_x, point[1])], color=GROUND)
        dot(ground_bus_x, point[1], color=GROUND)

    # I2C signals and pull-ups.
    wire([esp["SDA"], pn["SDA"]], color=I2C)
    wire([esp["SCL"], pn["SCL"]], color=I2C)
    resistor_vertical(610, power_y, esp["SDA"][1])
    resistor_vertical(660, power_y, esp["SCL"][1])
    dot(610, power_y, color=POWER)
    dot(660, power_y, color=POWER)
    dot(610, esp["SDA"][1], color=I2C)
    dot(660, esp["SCL"][1], color=I2C)
    label("I2C pull-ups\n4.7k to 3.3V", 700, 170, size=14, anchor="start")

    # TM1637 signals.
    wire([esp["CLK"], tm["CLK"]], color=DISPLAY)
    wire([esp["DIO"], tm["DIO"]], color=DISPLAY)

    # Status LEDs: GPIO -> series resistor -> LED -> GND.
    label("Status LEDs", 670, 560, size=18, weight="600")
    label("GPIO -> resistor -> LED -> GND", 670, 585, size=13, color=MUTED)
    wire([esp["RED"], (535, esp["RED"][1])], color=LED)
    resistor_horizontal(535, 635, esp["RED"][1], color=LED)
    wire([(635, esp["RED"][1]), (675, esp["RED"][1])], color=LED)
    led_symbol(675, esp["RED"][1], color=LED)
    wire([(719, esp["RED"][1]), (ground_bus_x, esp["RED"][1])], color=GROUND)
    dot(ground_bus_x, esp["RED"][1], color=GROUND)
    label("red", 770, esp["RED"][1] - 14, size=13, color=MUTED, anchor="start")

    wire([esp["GREEN"], (535, esp["GREEN"][1])], color=LED)
    resistor_horizontal(535, 635, esp["GREEN"][1], color=LED)
    wire([(635, esp["GREEN"][1]), (675, esp["GREEN"][1])], color=LED)
    led_symbol(675, esp["GREEN"][1], color=LED)
    wire([(719, esp["GREEN"][1]), (ground_bus_x, esp["GREEN"][1])], color=GROUND)
    dot(ground_bus_x, esp["GREEN"][1], color=GROUND)
    label("green", 770, esp["GREEN"][1] - 14, size=13, color=MUTED, anchor="start")

    # Activation input: external pull-up and a latching switch/button to GND.
    pullup_power_x = 805
    pullup_node_x = 940
    pullup_resistor_end_x = 905
    pullup_y = 745
    vertical_wire_with_jumps(
        pullup_power_x,
        power_y,
        pullup_y,
        jumps=[esp["SDA"][1], esp["SCL"][1], esp["CLK"][1], esp["DIO"][1], esp["RED"][1], esp["GREEN"][1]],
        color=POWER,
    )
    dot(pullup_power_x, power_y, color=POWER)
    resistor_horizontal(pullup_power_x, pullup_resistor_end_x, pullup_y, color=POWER, right_color=SWITCH)
    wire([(pullup_resistor_end_x, pullup_y), (pullup_node_x, pullup_y), (pullup_node_x, esp["SWITCH"][1])], color=SWITCH)
    wire([esp["SWITCH"], button], color=SWITCH)
    dot(pullup_node_x, esp["SWITCH"][1], color=SWITCH)
    label("external pull-up 10k", 870, pullup_y - 20, size=12, color=SWITCH)
    latching_control_to_ground(*button, ground_y)
    dot(button[0] + 100, ground_y, color=GROUND)

    # PN532 reset is intentionally left open in the current wiring.
    line(pn["RST"][0], pn["RST"][1], pn["RST"][0], pn["RST"][1] + 24, color="#9ca3af", width=2)
    label("reset pin not connected", pn["RST"][0] + 18, pn["RST"][1] + 28, size=12, color=MUTED, anchor="start")

    add("</svg>")
    OUT.write_text("\n".join(parts) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
