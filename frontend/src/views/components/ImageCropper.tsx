import { createResourceUrl, revokeResourceUrl } from "../../lib/remote/remoteResources";
import { useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";

export type ImageCropShape = "circle" | "square" | "banner";

type ImageCropperProps = {
  file: File;
  onCancel: () => void;
  onCropped: (file: File) => void;
  /** Avatars crop to a circle, room icons to a square, banners to a wide strip. */
  shape?: ImageCropShape;
};

// Output size per shape. The aspect ratio also drives the preview frame.
const SHAPE_OUTPUT: Record<ImageCropShape, { width: number; height: number }> = {
  circle: { width: 512, height: 512 },
  square: { width: 512, height: 512 },
  banner: { width: 960, height: 384 },
};

const MIN_SCALE = 1;
const MAX_SCALE = 4;
const INITIAL_SCALE = 1;

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    const timeoutId = window.setTimeout(() => reject(new Error("이미지를 불러오지 못했어요.")), 8000);
    image.addEventListener("load", () => {
      window.clearTimeout(timeoutId);
      resolve(image);
    });
    image.addEventListener("error", () => {
      window.clearTimeout(timeoutId);
      reject(new Error("이미지를 불러오지 못했어요."));
    });
    image.src = src;
  });
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

/**
 * The crop rectangle in source pixels. At scale 1 it is the largest rectangle of
 * the frame's aspect ratio that fits the image (like CSS `cover`); offsets move it
 * across the remaining free space, -50 to 50 percent of that space.
 */
function cropRect(
  natural: { width: number; height: number },
  aspect: number,
  scale: number,
  offsetX: number,
  offsetY: number
) {
  const baseWidth = Math.min(natural.width, natural.height * aspect);
  const width = baseWidth / scale;
  const height = width / aspect;
  const freeX = Math.max(0, natural.width - width);
  const freeY = Math.max(0, natural.height - height);
  return {
    x: clamp(freeX / 2 + (offsetX / 100) * freeX, 0, freeX),
    y: clamp(freeY / 2 + (offsetY / 100) * freeY, 0, freeY),
    width,
    height,
  };
}

export default function ImageCropper({ file, onCancel, onCropped, shape = "circle" }: ImageCropperProps) {
  const previewRef = useRef<HTMLDivElement | null>(null);
  const pointersRef = useRef<Map<number, { x: number; y: number }>>(new Map());
  const pinchDistanceRef = useRef(0);
  const [objectUrl, setObjectUrl] = useState("");
  const [natural, setNatural] = useState<{ width: number; height: number } | null>(null);
  const [scale, setScale] = useState(INITIAL_SCALE);
  const [offsetX, setOffsetX] = useState(0);
  const [offsetY, setOffsetY] = useState(0);
  const [dragging, setDragging] = useState(false);
  const [status, setStatus] = useState("");
  const output = SHAPE_OUTPUT[shape];
  const aspect = output.width / output.height;

  useEffect(() => {
    const url = createResourceUrl(file);
    let cancelled = false;
    setObjectUrl(url);
    setNatural(null);
    setStatus("");
    setScale(INITIAL_SCALE);
    setOffsetX(0);
    setOffsetY(0);
    loadImage(url)
      .then((image) => {
        if (!cancelled) setNatural({ width: image.naturalWidth, height: image.naturalHeight });
      })
      .catch((error: unknown) => {
        if (!cancelled) setStatus(error instanceof Error ? error.message : "이미지를 불러오지 못했어요.");
      });
    return () => {
      cancelled = true;
      revokeResourceUrl(url);
    };
  }, [file]);

  // Wheel zoom needs a non-passive listener so the page doesn't scroll/zoom.
  useEffect(() => {
    const element = previewRef.current;
    if (!element) return;
    function handleWheel(event: WheelEvent) {
      event.preventDefault();
      const step = event.deltaY * -0.0025;
      setScale((previous) => clamp(previous * (1 + step), MIN_SCALE, MAX_SCALE));
    }
    element.addEventListener("wheel", handleWheel, { passive: false });
    return () => element.removeEventListener("wheel", handleWheel);
  }, []);

  // The preview draws the same rectangle the canvas will export: the image is sized
  // so the crop rectangle fills the frame, and positioned by the same offsets.
  const rect = natural ? cropRect(natural, aspect, scale, offsetX, offsetY) : null;
  const previewStyle = {
    aspectRatio: `${output.width} / ${output.height}`,
    backgroundImage: objectUrl && natural ? `url("${objectUrl}")` : undefined,
    backgroundSize: rect && natural ? `${(natural.width / rect.width) * 100}% auto` : undefined,
    backgroundPosition: `${50 + offsetX}% ${50 + offsetY}%`,
    cursor: dragging ? "grabbing" : "grab",
    touchAction: "none" as const,
  };

  function applyDrag(dx: number, dy: number) {
    const bounds = previewRef.current?.getBoundingClientRect();
    const width = bounds?.width || 200;
    const height = bounds?.height || 200;
    // Dragging follows the finger: moving right shows more of the left side.
    setOffsetX((previous) => clamp(previous - (dx / width) * 100, -50, 50));
    setOffsetY((previous) => clamp(previous - (dy / height) * 100, -50, 50));
  }

  function pinchDistance(): number {
    const points = Array.from(pointersRef.current.values());
    if (points.length < 2) return 0;
    return Math.hypot(points[0].x - points[1].x, points[0].y - points[1].y);
  }

  function handlePointerDown(event: ReactPointerEvent<HTMLDivElement>) {
    event.currentTarget.setPointerCapture(event.pointerId);
    pointersRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    pinchDistanceRef.current = pinchDistance();
    setDragging(true);
  }

  function handlePointerMove(event: ReactPointerEvent<HTMLDivElement>) {
    const previous = pointersRef.current.get(event.pointerId);
    if (!previous) return;
    pointersRef.current.set(event.pointerId, { x: event.clientX, y: event.clientY });
    if (pointersRef.current.size >= 2) {
      const distance = pinchDistance();
      if (pinchDistanceRef.current > 0 && distance > 0) {
        const ratio = distance / pinchDistanceRef.current;
        setScale((current) => clamp(current * ratio, MIN_SCALE, MAX_SCALE));
      }
      pinchDistanceRef.current = distance;
      return;
    }
    applyDrag(event.clientX - previous.x, event.clientY - previous.y);
  }

  function handlePointerEnd(event: ReactPointerEvent<HTMLDivElement>) {
    pointersRef.current.delete(event.pointerId);
    pinchDistanceRef.current = pinchDistance();
    if (pointersRef.current.size === 0) setDragging(false);
  }

  function reset() {
    setScale(INITIAL_SCALE);
    setOffsetX(0);
    setOffsetY(0);
  }

  async function cropImage() {
    if (!objectUrl) return;
    setStatus("이미지 처리 중...");
    try {
      const sourceImage = await loadImage(objectUrl);
      const canvas = document.createElement("canvas");
      canvas.width = output.width;
      canvas.height = output.height;
      const context = canvas.getContext("2d");
      if (!context) throw new Error("이미지 편집 캔버스를 사용할 수 없어요.");
      const source = cropRect(
        { width: sourceImage.naturalWidth, height: sourceImage.naturalHeight },
        aspect,
        scale,
        offsetX,
        offsetY
      );
      context.clearRect(0, 0, canvas.width, canvas.height);
      if (shape === "circle") {
        context.save();
        context.beginPath();
        context.arc(output.width / 2, output.height / 2, output.width / 2, 0, Math.PI * 2);
        context.clip();
      }
      context.drawImage(
        sourceImage,
        source.x,
        source.y,
        source.width,
        source.height,
        0,
        0,
        output.width,
        output.height
      );
      if (shape === "circle") context.restore();
      let completed = false;
      const timeoutId = window.setTimeout(() => {
        if (!completed) setStatus("이미지 처리 실패");
      }, 8000);
      canvas.toBlob((blob) => {
        completed = true;
        window.clearTimeout(timeoutId);
        if (!blob) {
          setStatus("이미지 처리 실패");
          return;
        }
        setStatus("");
        onCropped(new File([blob], `${shape}-${Date.now()}.png`, { type: "image/png" }));
      }, "image/png");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "이미지 처리 실패");
    }
  }

  return (
    <div className="dc-image-cropper" data-shape={shape}>
      <div
        ref={previewRef}
        className="dc-image-crop-preview"
        style={previewStyle}
        aria-label="사진 미리보기 (드래그로 이동, 휠·핀치·슬라이더로 확대)"
        role="img"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerEnd}
        onPointerCancel={handlePointerEnd}
      />
      <label className="dc-image-crop-zoom">
        <span className="sr-only">확대</span>
        <span aria-hidden className="dc-image-crop-zoom-small" />
        <input
          type="range"
          min={MIN_SCALE}
          max={MAX_SCALE}
          step={0.01}
          value={scale}
          onChange={(event) => setScale(Number(event.currentTarget.value))}
        />
        <span aria-hidden className="dc-image-crop-zoom-large" />
      </label>
      <div className="dc-image-crop-actions">
        <button type="button" className="dc-image-crop-reset" onClick={reset}>
          초기화
        </button>
        <span className="flex-1" />
        <button type="button" className="ops-button" onClick={onCancel}>
          취소
        </button>
        <button type="button" className="ops-cta min-h-11 px-4" disabled={!natural} onClick={cropImage}>
          적용
        </button>
      </div>
      {status && <p className="dc-member-session-status preserve-words" role="status">{status}</p>}
    </div>
  );
}
