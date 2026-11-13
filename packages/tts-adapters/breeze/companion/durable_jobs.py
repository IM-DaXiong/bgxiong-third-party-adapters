"""Durable local speech admission. No model imports and no inference inside transactions."""
from __future__ import annotations

import copy
from contextlib import contextmanager
import hashlib
import json
import logging
import os
from pathlib import Path
import re
import sqlite3
import threading
import time
import uuid


class JobError(RuntimeError):
    def __init__(self, code: str, message: str):
        super().__init__(message)
        self.code = code


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def sha_file(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def atomic_bytes(path: Path, body: bytes):
    temporary = path.with_name(path.name + "." + uuid.uuid4().hex + ".part")
    try:
        with temporary.open("xb") as output:
            output.write(body)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


class Jobs:
    def __init__(self, root: Path, execute, reference_root: Path):
        self.root = Path(root).resolve()
        self.root.mkdir(parents=True, exist_ok=True)
        self.reference_root = Path(reference_root).resolve()
        self.execute = execute
        self.lock = threading.RLock()
        self.wake = threading.Event()
        self.stop_requested = threading.Event()
        self.worker = None
        self.instance_id = uuid.uuid4().hex
        self.owner = (self.root / "owner.lock").open("a+b")
        try:
            self.owner.seek(0)
            if self.owner.read(1) == b"":
                self.owner.write(b"0"); self.owner.flush()
            self.owner.seek(0)
            if os.name == "nt":
                import msvcrt
                msvcrt.locking(self.owner.fileno(), msvcrt.LK_NBLCK, 1)
            else:
                import fcntl
                fcntl.flock(self.owner.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError as error:
            self.owner.close()
            raise JobError("STORE_BUSY", "Another companion owns this job store") from error
        self.database = self.root / "jobs.sqlite3"
        try:
            with self.connect() as db:
                db.executescript("CREATE TABLE IF NOT EXISTS metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);"
                                 "CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY,digest TEXT NOT NULL,state TEXT NOT NULL,"
                                 "document TEXT NOT NULL,cancel_requested INTEGER NOT NULL DEFAULT 0,updated REAL NOT NULL);")
                db.execute("INSERT OR IGNORE INTO metadata VALUES('storeId',?)", (uuid.uuid4().hex,))
                self.store_id = db.execute("SELECT value FROM metadata WHERE key='storeId'").fetchone()[0]
                # Exclusive process lock proves no old worker can still publish an output.
                for identifier, raw in db.execute("SELECT id,document FROM jobs WHERE state='running'").fetchall():
                    doc = json.loads(raw)
                    doc['failure'] = 'Original companion stopped before committing its result'
                    db.execute("UPDATE jobs SET state='failed',document=?,updated=? WHERE id=?",
                               (canonical(doc), time.time(), identifier))
        except BaseException:
            self.owner.close()
            raise

    @contextmanager
    def connect(self):
        db = sqlite3.connect(self.database, timeout=5)
        try:
            db.execute("PRAGMA synchronous=FULL")
            with db:
                yield db
        finally:
            db.close()

    def identity(self):
        return {"storeId": self.store_id, "instanceId": self.instance_id, "jobProtocol": "durable-speech-v1"}

    def _identity(self, request_id, digest, expected_store):
        if expected_store != self.store_id:
            raise JobError("STORE_IDENTITY_CHANGED", "Original durable job store is unavailable")
        if not isinstance(request_id, str) or not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}", request_id):
            raise JobError("INVALID_REQUEST", "Invalid SDK request identity")
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise JobError("INVALID_REQUEST", "Invalid SDK request digest")

    def _row(self, identifier):
        with self.connect() as db:
            row = db.execute("SELECT digest,state,document,cancel_requested FROM jobs WHERE id=?", (identifier,)).fetchone()
        return None if row is None else (row[0], row[1], json.loads(row[2]), bool(row[3]))

    def _public(self, identifier, row):
        digest, state, document, cancelled = row
        return {**self.identity(), "jobId": identifier, "requestId": identifier,
                "requestDigest": digest, "status": state, "cancelRequested": cancelled,
                "receipt": document.get('receipt'), "audioSha256": document.get('audioSha256'),
                "audioBytes": document.get('audioBytes'), "failure": document.get('failure'),
                "tombstoneId": document.get('tombstoneId')}

    def submit(self, request_id, digest, expected_store, speech, receipt):
        self._identity(request_id, digest, expected_store)
        if not isinstance(speech, dict) or not isinstance(receipt, dict):
            raise JobError("INVALID_REQUEST", "Speech and receipt must be objects")
        with self.lock:
            existing = self._row(request_id)
            if existing:
                if existing[0] != digest:
                    raise JobError("REQUEST_CONFLICT", "Request identity is already bound to another input")
                if existing[1] == 'rejected':
                    raise JobError("REQUEST_REJECTED", "A durable tombstone forbids late submission")
                return self._public(request_id, existing)
            directory = self.root / request_id
            directory.mkdir(exist_ok=True)
            speech = copy.deepcopy(speech)
            if speech.get('reference'):
                original = Path(speech['reference']['audioPath']).resolve(strict=True)
                if not original.is_relative_to(self.reference_root) or not original.is_file():
                    raise JobError("REFERENCE_OUTSIDE_ROOT", "Reference must be an uploaded local snapshot")
                # File copying precedes the short metadata transaction.
                reference = directory / ('reference' + original.suffix.lower())
                atomic_bytes(reference, original.read_bytes())
                speech['reference']['audioPath'] = str(reference)
                reference_sha = sha_file(reference)
            else:
                reference_sha = None
            speech['requestId'] = request_id
            document = {'speech': speech, 'receipt': copy.deepcopy(receipt), 'referenceSha256': reference_sha}
            with self.connect() as db:
                db.execute("INSERT INTO jobs(id,digest,state,document,updated) VALUES(?,?,'queued',?,?)",
                           (request_id, digest, canonical(document), time.time()))
            self.wake.set()
            return self._public(request_id, (digest, 'queued', document, False))

    def resolve(self, request_id, digest, expected_store):
        if expected_store != self.store_id:
            return {**self.identity(), 'outcome': 'unknown', 'reason': 'storeIdentityChanged'}
        self._identity(request_id, digest, expected_store)
        with self.lock:
            row = self._row(request_id)
            if row and row[0] != digest:
                raise JobError("REQUEST_CONFLICT", "Original input digest does not match")
            if row is None:
                document = {'tombstoneId': uuid.uuid4().hex}
                with self.connect() as db:
                    db.execute("INSERT INTO jobs(id,digest,state,document,updated) VALUES(?,?,'rejected',?,?)",
                               (request_id, digest, canonical(document), time.time()))
                row = (digest, 'rejected', document, False)
            result = self._public(request_id, row)
            result['outcome'] = 'rejected' if row[1] == 'rejected' else 'accepted'
            result['fencesLateSubmit'] = row[1] == 'rejected'
            return result

    def get(self, identifier):
        with self.lock:
            row = self._row(identifier)
            if not row: raise JobError("JOB_NOT_FOUND", "Job is unknown; absence is not a rejection receipt")
            return self._public(identifier, row)

    def cancel(self, identifier, digest, expected_store):
        self._identity(identifier, digest, expected_store)
        with self.lock:
            row = self._row(identifier)
            if not row: raise JobError("JOB_NOT_FOUND", "Job does not exist")
            if row[0] != digest: raise JobError("REQUEST_CONFLICT", "Cancellation input identity differs")
            if row[1] in ('queued', 'running'):
                with self.connect() as db:
                    db.execute("UPDATE jobs SET cancel_requested=1,state=?,updated=? WHERE id=?",
                               ('cancelled' if row[1] == 'queued' else 'running', time.time(), identifier))
            return self.get(identifier)

    def audio(self, identifier):
        value = self.get(identifier)
        if value['status'] != 'succeeded': raise JobError("AUDIO_NOT_READY", "No committed result")
        path = self.root / identifier / 'audio.wav'
        if not path.is_file() or path.stat().st_size != value['audioBytes'] or sha_file(path) != value['audioSha256']:
            raise JobError("AUDIO_CHANGED", "Committed result is missing or changed")
        return path

    def run_one(self):
        with self.lock:
            with self.connect() as db:
                selected = db.execute("SELECT id,digest,document FROM jobs WHERE state='queued' ORDER BY updated LIMIT 1").fetchone()
                if not selected: return False
                identifier, digest, raw = selected
                db.execute("UPDATE jobs SET state='running',updated=? WHERE id=?", (time.time(), identifier))
        document = json.loads(raw)
        try:
            if document.get('referenceSha256') and sha_file(document['speech']['reference']['audioPath']) != document['referenceSha256']:
                raise JobError("REFERENCE_CHANGED", "Frozen reference changed")
            wav = self.execute(copy.deepcopy(document['speech']))
            if not isinstance(wav, bytes) or len(wav) < 44: raise JobError("INVALID_AUDIO", "No complete WAV was produced")
            destination = self.root / identifier / 'audio.wav'
            atomic_bytes(destination, wav)
            document.update(audioSha256=sha_file(destination), audioBytes=len(wav))
            state = 'succeeded'
        except Exception as error:
            document['failure'] = str(error)
            state = 'failed'
        # A storage outage after inference must not kill the worker or re-run the
        # model. Keep the finished receipt in this attempt until it is committed.
        while True:
            try:
                with self.lock:
                    row = self._row(identifier)
                    # Breeze cannot preempt a running kernel. A requested cancel
                    # must not erase an actual successful result or invent failure.
                    terminal = state
                    with self.connect() as db:
                        db.execute("UPDATE jobs SET state=?,document=?,updated=? WHERE id=? AND state='running' AND digest=?",
                                   (terminal, canonical(document), time.time(), identifier, digest))
                return True
            except (OSError, sqlite3.Error):
                logging.exception("Could not commit completed speech job %s; preserving worker ownership", identifier)
                time.sleep(.5)

    def start(self):
        def loop():
            while not self.stop_requested.is_set():
                try:
                    if not self.run_one(): self.wake.wait(.5); self.wake.clear()
                except (OSError, sqlite3.Error):
                    logging.exception("Could not read speech job queue; preserving worker ownership")
                    self.wake.wait(.5); self.wake.clear()
        self.worker = threading.Thread(target=loop, name='breeze-durable-jobs', daemon=True)
        self.worker.start()

    def close(self):
        self.stop_requested.set(); self.wake.set()
        if self.worker:
            self.worker.join()  # Do not release ownership while inference is alive.
        if not self.owner.closed: self.owner.close()
