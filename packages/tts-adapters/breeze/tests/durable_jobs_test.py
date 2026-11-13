import concurrent.futures
import importlib.util
import io
import json
from pathlib import Path
import sqlite3
import tempfile
import threading
import unittest
import wave

spec=importlib.util.spec_from_file_location('durable_jobs',Path(__file__).resolve().parents[1]/'companion/durable_jobs.py')
module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)

def wav():
    output=io.BytesIO()
    with wave.open(output,'wb') as audio:
        audio.setnchannels(1); audio.setsampwidth(2); audio.setframerate(24000); audio.writeframes(bytes(4800))
    return output.getvalue()

class DurableTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name);self.refs=self.root/'refs';self.refs.mkdir()
        self.count=0
        def execute(body):self.count+=1;return wav()
        self.jobs=module.Jobs(self.root/'jobs',execute,self.refs)
    def tearDown(self):self.jobs.close();self.temp.cleanup()
    def submit(self,identifier='request',digest='a'*64,speech=None):
        return self.jobs.submit(identifier,digest,self.jobs.store_id,speech or {'input':'hello','mode':'plain'},{'audit':'preserved'})
    def test_concurrent_same_request_runs_once_and_identity_conflicts_fail(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            values=list(pool.map(lambda _:self.submit(),range(8)))
        self.assertTrue(all(v['jobId']=='request' for v in values))
        with self.assertRaises(module.JobError):self.submit(digest='b'*64)
        self.assertTrue(self.jobs.run_one());self.assertFalse(self.jobs.run_one());self.assertEqual(self.count,1)
        self.assertEqual(self.jobs.get('request')['status'],'succeeded');self.assertEqual(self.jobs.audio('request').read_bytes(),wav())
    def test_tombstone_fences_late_post_and_changed_store_never_rejects(self):
        value=self.jobs.resolve('request','a'*64,self.jobs.store_id)
        self.assertEqual(value['outcome'],'rejected');self.assertTrue(value['fencesLateSubmit'])
        with self.assertRaises(module.JobError):self.submit()
        unknown=self.jobs.resolve('other','a'*64,'missing-store');self.assertEqual(unknown['outcome'],'unknown')
        with self.assertRaises(module.JobError):self.jobs.get('other')
        self.assertFalse(self.jobs.run_one())
    def test_submit_and_resolution_race_never_leaves_rejected_active_job(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            submit=pool.submit(self.submit)
            resolve=pool.submit(self.jobs.resolve,'request','a'*64,self.jobs.store_id)
            value=resolve.result()
            try:accepted=submit.result()
            except module.JobError:accepted=None
        if value['outcome']=='rejected':self.assertIsNone(accepted);self.assertFalse(self.jobs.run_one())
        else:self.assertEqual(accepted['jobId'],'request');self.assertTrue(self.jobs.run_one())
    def test_reference_snapshot_and_result_survive_restart(self):
        ref=self.refs/'source.wav';ref.write_bytes(wav())
        self.submit(speech={'input':'hello','mode':'reference_clone','reference':{'audioPath':str(ref),'promptText':'hello'}})
        ref.unlink();self.jobs.run_one();identity=self.jobs.identity();digest=self.jobs.get('request')['audioSha256']
        self.jobs.close();self.jobs=module.Jobs(self.root/'jobs',lambda _:self.fail('must not regenerate'),self.refs)
        self.assertEqual(self.jobs.store_id,identity['storeId']);self.assertNotEqual(self.jobs.instance_id,identity['instanceId'])
        self.assertEqual(self.jobs.get('request')['audioSha256'],digest);self.assertEqual(self.jobs.audio('request').read_bytes(),wav())
        self.assertFalse(self.jobs.run_one())
    def test_cancel_waits_for_running_worker_and_interrupted_is_terminal(self):
        started=threading.Event();finish=threading.Event()
        def execute(_):started.set();finish.wait(5);return wav()
        self.jobs.execute=execute;self.submit()
        worker=threading.Thread(target=self.jobs.run_one);worker.start();self.assertTrue(started.wait(2))
        try:
            with self.assertRaises(module.JobError):self.jobs.cancel('request','a'*64,'replacement-store')
            with self.assertRaises(module.JobError):self.jobs.cancel('request','b'*64,self.jobs.store_id)
            self.assertFalse(self.jobs.get('request')['cancelRequested'])
            self.assertEqual(self.jobs.cancel('request','a'*64,self.jobs.store_id)['status'],'running')
        finally:
            finish.set();worker.join()
        self.assertEqual(self.jobs.get('request')['status'],'succeeded')
        self.assertTrue(self.jobs.get('request')['cancelRequested'])
        self.assertEqual(self.jobs.audio('request').read_bytes(),wav())
        self.submit('queued-cancel')
        self.assertEqual(self.jobs.cancel('queued-cancel','a'*64,self.jobs.store_id)['status'],'cancelled')
        self.assertFalse(self.jobs.run_one())
        self.submit('interrupted')
        with self.jobs.connect() as db:db.execute("UPDATE jobs SET state='running' WHERE id='interrupted'")
        self.jobs.close();self.jobs=module.Jobs(self.root/'jobs',lambda _:self.fail('no invisible restart'),self.refs)
        self.assertEqual(self.jobs.get('interrupted')['status'],'failed');self.assertFalse(self.jobs.run_one())
    def test_commit_outage_retains_result_without_executing_twice(self):
        self.submit()
        original=self.jobs._row
        failures=0
        def interrupted_read(identifier):
            nonlocal failures
            if self.count and failures == 0:
                failures+=1
                raise sqlite3.OperationalError('controlled terminal commit outage')
            return original(identifier)
        self.jobs._row=interrupted_read
        with self.assertLogs(level='ERROR'):
            self.assertTrue(self.jobs.run_one())
        self.assertEqual(failures,1);self.assertEqual(self.count,1)
        self.assertEqual(self.jobs.get('request')['status'],'succeeded')
        self.assertFalse(self.jobs.run_one())
    def test_store_owner_excludes_second_process_and_output_digest_is_checked(self):
        with self.assertRaises(module.JobError):module.Jobs(self.root/'jobs',lambda _:wav(),self.refs)
        self.submit();self.jobs.run_one();self.jobs.audio('request').write_bytes(b'changed')
        with self.assertRaises(module.JobError):self.jobs.audio('request')

if __name__=='__main__':unittest.main()
