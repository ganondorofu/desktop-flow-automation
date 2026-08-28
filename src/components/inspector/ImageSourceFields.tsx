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
          <img src={`data:image/png;base64,${image.data}`} alt="" className="insp-image-thumb" />
          <span>{t("inspector.fields.imageEmbedded")}</span>
        </div>
      ) : (
        <>
          <input
            className="num-input"
            value={image.value}
            onChange={(e) => onChangeImage({ kind: "path", value: e.target.value })}
          />
          {pathPreview && <img src={`data:image/png;base64,${pathPreview}`} alt="" className="insp-image-thumb" />}
        </>
      )}
      <div className="insp-image-actions">
        <CaptureRegionButton onCaptured={(data) => onChangeImage({ kind: "embedded", data })} />
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
