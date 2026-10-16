"""Regenerate the checked-in Windows icon. Requires Pillow only at development time."""

from pathlib import Path

from PIL import Image, ImageDraw


SCALE = 4
SIZE = 256
CANVAS = SIZE * SCALE
ASSETS = Path(__file__).resolve().parents[1] / "assets"


def point(x: int, y: int) -> tuple[int, int]:
    return x * SCALE, y * SCALE


def main() -> None:
    image = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    gradient = Image.new("RGBA", image.size)
    pixels = gradient.load()
    top = (45, 107, 226)
    bottom = (21, 43, 126)
    for y in range(CANVAS):
        t = y / (CANVAS - 1)
        color = tuple(round(a * (1 - t) + b * t) for a, b in zip(top, bottom))
        for x in range(CANVAS):
            pixels[x, y] = (*color, 255)

    mask = Image.new("L", image.size)
    mask_draw = ImageDraw.Draw(mask)
    mask_draw.rounded_rectangle((*point(10, 10), *point(246, 246)), radius=56 * SCALE, fill=255)
    image.paste(gradient, (0, 0), mask)

    draw = ImageDraw.Draw(image)
    draw.rounded_rectangle(
        (*point(12, 12), *point(244, 244)),
        radius=54 * SCALE,
        outline=(255, 255, 255, 44),
        width=3 * SCALE,
    )
    # A broad M reads clearly even at the 16 px taskbar size.
    letter = [
        (51, 188), (51, 68), (78, 68), (128, 128), (178, 68),
        (205, 68), (205, 188), (174, 188), (174, 113), (141, 154),
        (115, 154), (82, 113), (82, 188),
    ]
    draw.polygon([point(x, y) for x, y in letter], fill=(255, 255, 255, 255))
    draw.rounded_rectangle(
        (*point(92, 207), *point(164, 215)),
        radius=4 * SCALE,
        fill=(117, 231, 218, 255),
    )

    icon = image.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    ASSETS.mkdir(exist_ok=True)
    icon.save(ASSETS / "app-icon.png")
    icon.save(
        ASSETS / "app-icon.ico",
        format="ICO",
        sizes=[(size, size) for size in (16, 24, 32, 48, 64, 128, 256)],
    )


if __name__ == "__main__":
    main()
