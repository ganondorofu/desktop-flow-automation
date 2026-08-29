import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type { ImageSourceField } from "../../data/flowModel";
import { CaptureRegionButton } from "./pickers";
import { ImageCropTool } from "./ImageCropTool";

const IMAGE_FILTERS = [{ name: "Image", extensions: ["png", "jpg", "jpeg", "bmp", "gif", "webp"] }];

/** The reference-image field for `find_image` steps. `embedded` (the
 *  common case — capturing or browsing both produce this) shows a
 *  thumbnail preview instead of an editable value, since the base64
 *  data itself isn't something a user edits by hand; `path` shows the
 *  plain text field it always has, plus a best-effort preview loaded
 *  from disk. Mirrors `flow_schema::ImageSource`. Browsing an existing
 *  file offers a crop step (`ImageCropTool`) before it's embedded, so
 *  a whole screenshot can be trimmed down to just the relevant icon
 *  instead of only usable as-is. */
export function ImageSourceFields({
  image,
  onChangeImage,
}: {
  image: ImageSourceField;
  onChangeImage: (next: ImageSourceField) => void;
}) {
  const { t } = useTranslation();
  const [pathPreview, setPathPreview] = useState<string | null>(null);
  const [pendingCrop, setPendingCrop] = useState<string | null>(null);
  const [browseFailed, setBrowseFailed] = useState(false);
  // Full-size preview base64 (embedded or path-loaded, whichever the
  // user clicked) — the 40px thumbnail below crops non-square images
  // via `object-fit: cover`, so most of a wide/tall reference image
  // is otherwise never actually visible.
  const [lightbox, setLightbox] = useState<string | null>(null);

  useEffect(() => {
    if (!lightbox) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setLightbox(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [lightbox]);

  useEffect(() => {
    setPathPreview(null);
    if (image.kind !== "path" || !image.value) return;
    let cancelled = false;
    invoke<string>("read_file_base64", { path: image.value })
      .then((data) => {
        if (!cancelled) setPathPreview(data);
      })
      .catch(() => {
        if (!cancelled) setPathPreview(null);
      });
    return () => {
      cancelled = true;
    };
  }, [image]);

  async function browse() {
    setBrowseFailed(false);
    const path = await openDialog({ multiple: false, directory: false, filters: IMAGE_FILTERS });
    if (typeof path !== "string") return;
    try {
      const data = await invoke<string>("read_file_base64", { path });
      setPendingCrop(data);
    } catch {
      setBrowseFailed(true);
    }
  }

  if (pendingCrop) {
    return (
      <ImageCropTool
        base64Png={pendingCrop}
        onCropped={(data) => {
          setPendingCrop(null);
          onChangeImage({ kind: "embedded", data });
        }}
        onUseWholeImage={() => {
          onChangeImage({ kind: "embedded", data: pendingCrop });
          setPendingCrop(null);
        }}
      />
    );
  }

  return (
    <div className="field">
      <label>{t("inspector.fields.image")}</label>
      {image.kind === "embedded" ? (
        <div className="insp-image-embedded">
          <button type="button" className="insp-image-thumb-btn" onClick={() => setLightbox(image.data)} title={t("inspector.fields.imageViewFull")}>
            <img src={`data:image/png;base64,${image.data}`} alt="" className="insp-image-thumb" />
          </button>
          <span>{t("inspector.fields.imageEmbedded")}</span>
        </div>
      ) : (
        <>
          <input
            className="num-input"
            value={image.value}
            onChange={(e) => onChangeImage({ kind: "path", value: e.target.value })}
          />
          {pathPreview && (
            <button type="button" className="insp-image-thumb-btn" onClick={() => setLightbox(pathPreview)} title={t("inspector.fields.imageViewFull")}>
              <img src={`data:image/png;base64,${pathPreview}`} alt="" className="insp-image-thumb" />
            </button>
          )}
        </>
      )}
      {lightbox && (
        <div className="image-lightbox-backdrop" onClick={() => setLightbox(null)}>
          <img src={`data:image/png;base64,${lightbox}`} alt="" className="image-lightbox-img" />
        </div>
      )}
      <div className="insp-image-actions">
        <CaptureRegionButton onCaptured={(data, capturedScale) => onChangeImage({ kind: "embedded", data, capturedScale })} />
        <button type="button" className="insp-record-btn" onClick={() => void browse()}>
          {t("inspector.fields.imageBrowse")}
        </button>
        {image.kind === "embedded" && (
          <button type="button" className="insp-cancel-btn" onClick={() => onChangeImage({ kind: "path", value: "" })}>
            {t("inspector.fields.imageUsePath")}
          </button>
        )}
      </div>
      {browseFailed && <span className="insp-capture-status insp-capture-error">{t("inspector.fields.imageBrowseFailed")}</span>}
    </div>
  );
}
