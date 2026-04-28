#!/bin/sh
# Blur-lock: per-output screenshot, blur, lock
tmpdir=$(mktemp -d)
trap 'rm -rf "$tmpdir"' EXIT

args=""
for out in $(wlr-randr 2>/dev/null | awk '/^[^ \t]/ {print $1}'); do
    grim -l 0 -o "$out" "$tmpdir/$out.png" || continue
    ffmpeg -y -i "$tmpdir/$out.png" -vf "boxblur=8:2" "$tmpdir/$out-blur.png" 2>/dev/null
    args="$args -i $out:$tmpdir/$out-blur.png"
done

# Fallback to whole-canvas grab if per-output enumeration failed
if [ -z "$args" ]; then
    grim -l 0 "$tmpdir/all.png"
    ffmpeg -y -i "$tmpdir/all.png" -vf "boxblur=8:2" "$tmpdir/all-blur.png" 2>/dev/null
    args="-i $tmpdir/all-blur.png"
fi

swaylock -f $args
