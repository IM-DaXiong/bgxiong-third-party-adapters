"""Fixed 3D-Speaker models, bounded audio reads and resumable embedding batches."""
import hashlib
import json
import os
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "upstream"))
os.environ.update(HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1")


def atomic_json(path, value):
    path = Path(path)
    temporary = path.with_suffix(path.suffix + ".part")
    with temporary.open("w", encoding="utf-8") as stream:
        json.dump(value, stream, ensure_ascii=False)
        stream.flush()
        os.fsync(stream.fileno())
    os.replace(temporary, path)


class Cancelled(Exception):
    pass


class Engine:
    def __init__(self, model_root):
        import torch
        from funasr import AutoModel
        from speakerlab.bin import infer_diarization as upstream
        from speakerlab.process.cluster import CommonClustering
        torch.set_num_threads(4)
        model_root = Path(model_root)
        paths = {"iic/speech_campplus_sv_zh_en_16k-common_advanced": model_root / "campplus",
                 "iic/speech_fsmn_vad_zh-cn-16k-common-pytorch": model_root / "fsmn"}
        upstream.download_model_from_modelscope = lambda model, *a, **kw: str(paths[model])
        def vad(*a, **kw):
            instance = AutoModel(model=str(paths["iic/speech_fsmn_vad_zh-cn-16k-common-pytorch"]),
                                 device="cpu", disable_update=True, disable_pbar=True)
            return lambda wav: instance.generate(input=wav, disable_pbar=True)
        upstream.get_voice_activity_detection_model = vad
        upstream.get_cluster_backend = lambda: CommonClustering(cluster_type="AHC", mer_cos=0.8, min_cluster_size=4)
        self.native = upstream.Diarization3Dspeaker(device="cpu", include_overlap=False)

    def run(self, audio, spec, checkpoint, cancelled, progress):
        import numpy as np
        import soundfile as sf
        import torch
        checkpoint = Path(checkpoint)
        checkpoint.mkdir(parents=True, exist_ok=True)
        def check():
            if cancelled():
                raise Cancelled("USER_CANCELLED")
        with sf.SoundFile(audio) as source:
            if source.samplerate != 16000 or source.channels != 1:
                raise ValueError("DIARIZATION_EXPECTS_16K_MONO")
            duration = round(len(source) * 1000 / 16000)
            if abs(duration - spec["durationMs"]) > 150:
                raise ValueError("DIARIZATION_DURATION_MISMATCH")
            # VAD checkpoints use absolute source offsets; model windows never retime audio.
            chunks = []
            total_windows = (len(source) + 480000 - 1) // 480000
            for index in range(total_windows):
                check()
                cache = checkpoint / ("vad-%06d.json" % index)
                start = index * 480000
                if cache.exists():
                    turns = json.loads(cache.read_text(encoding="utf-8"))
                else:
                    source.seek(start)
                    wav = torch.from_numpy(source.read(480000, dtype="float32")).unsqueeze(0)
                    turns = [[a + start / 16000, b + start / 16000] for a, b in self.native.do_vad(wav)]
                    atomic_json(cache, turns)
                chunks.extend(c for a, b in turns for c in self.native.chunk(a, b) if c[1] > c[0])
                progress(0.2 * (index + 1) / max(1, total_windows), "vad")
            batches = []
            for index in range(0, len(chunks), 64):
                check()
                cache = checkpoint / ("embedding-%06d.npy" % index)
                group = chunks[index:index + 64]
                if cache.exists():
                    embeddings = np.load(cache, allow_pickle=False)
                    if embeddings.shape != (len(group), 192) or not np.isfinite(embeddings).all():
                        raise ValueError("DIARIZATION_CHECKPOINT_INVALID")
                else:
                    # Read individual speech windows, not the possibly hour-long span between them.
                    waves, local_chunks, cursor = [], [], 0
                    for a, b in group:
                        source.seek(int(a * 16000))
                        wav = source.read(int(b * 16000) - int(a * 16000), dtype="float32")
                        waves.append(wav)
                        local_chunks.append([cursor / 16000, (cursor + len(wav)) / 16000])
                        cursor += len(wav)
                    embeddings = self.native.do_emb_extraction(local_chunks, torch.from_numpy(np.concatenate(waves)).unsqueeze(0))
                    temporary = cache.with_suffix(".part")
                    with temporary.open("wb") as stream:
                        np.save(stream, embeddings, allow_pickle=False)
                        stream.flush()
                        os.fsync(stream.fileno())
                    os.replace(temporary, cache)
                batches.append(embeddings)
                progress(0.2 + 0.7 * min(index + 64, len(chunks)) / len(chunks), "embeddings")
        check()
        turns = []
        if batches:
            embeddings = np.concatenate(batches)
            if spec.get("speakerCount") is not None:
                # Same average-link cosine AHC as upstream, with the user's explicit
                # cluster-count stopping criterion (scipy), instead of a distance cut.
                from scipy.cluster.hierarchy import cut_tree
                from scipy.spatial.distance import squareform
                from sklearn.metrics.pairwise import cosine_similarity
                import fastcluster
                from speakerlab.bin.infer_diarization import compressed_seg
                count = spec["speakerCount"]
                if count > len(embeddings):
                    raise ValueError("DIARIZATION_TOO_FEW_SPEECH_WINDOWS")
                if count == 1:
                    labels = np.zeros(len(embeddings), dtype=int)
                else:
                    distances = squareform(np.maximum(0, 1 - cosine_similarity(embeddings)), checks=False)
                    tree = fastcluster.linkage(distances, method="average", preserve_input=False)
                    labels = cut_tree(tree, n_clusters=[count]).reshape(-1)
                turns = compressed_seg([[a, b, int(label)] for (a, b), label in zip(chunks, labels)])
            else:
                _, turns = self.native.do_clustering(chunks, embeddings, None)
        check()
        # Never silently ignore the user's count constraint.
        labels = list(dict.fromkeys(turn[2] for turn in turns))
        if spec.get("speakerCount") is not None and len(labels) != spec["speakerCount"]:
            raise ValueError("DIARIZATION_SPEAKER_COUNT_MISMATCH: detected %d; requested %d" % (len(labels), spec["speakerCount"]))
        names = {label: "speaker-%d" % (index + 1) for index, label in enumerate(labels)}
        result = {"schemaVersion": 1, "sourceFingerprint": spec["sourceFingerprint"],
                  "durationMs": spec["durationMs"], "speakers": [{"id": names[label]} for label in labels],
                  "turns": [{"startMs": max(0, round(a * 1000)), "endMs": min(spec["durationMs"], round(b * 1000)),
                             "speakerId": names[label]} for a, b, label in turns],
                  "overlapDetection": "unsupported", "warnings": ["DIARIZATION_OVERLAP_DETECTION_UNAVAILABLE"]}
        progress(1, "complete")
        return result
