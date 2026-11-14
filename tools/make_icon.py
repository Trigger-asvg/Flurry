# Генерирует assets/app.ico — чёрный квадрат (с едва заметной рамкой, чтобы был виден на тёмной панели задач).
import struct, os
def img(n):
    m = max(1, round(n * 0.06))            # поле
    px = bytearray()
    for y in range(n - 1, -1, -1):         # BMP снизу вверх
        for x in range(n):
            inside = m <= x < n - m and m <= y < n - m
            if not inside: px += bytes((0, 0, 0, 0)); continue
            edge = x in (m, n - m - 1) or y in (m, n - m - 1)
            v = 0x3a if edge else 0x00
            px += bytes((v, v, v, 255))
    mask = bytes(((n + 31) // 32 * 4) * n)
    hdr = struct.pack('<IiiHHIIiiII', 40, n, n * 2, 1, 32, 0, len(px) + len(mask), 0, 0, 0, 0)
    return hdr + bytes(px) + mask
sizes = [16, 20, 24, 32, 40, 48, 64, 128, 256]
blobs = [img(s) for s in sizes]
out = struct.pack('<HHH', 0, 1, len(sizes))
off = 6 + 16 * len(sizes)
for s, b in zip(sizes, blobs):
    out += struct.pack('<BBBBHHII', s % 256, s % 256, 0, 0, 1, 32, len(b), off); off += len(b)
out += b''.join(blobs)
p = os.path.join(os.path.dirname(__file__), '..', 'assets', 'app.ico')
open(p, 'wb').write(out); print('ok', len(out))
