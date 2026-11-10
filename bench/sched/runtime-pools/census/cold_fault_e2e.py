import argparse,importlib.util,sys
p=argparse.ArgumentParser(add_help=False,allow_abbrev=False);p.add_argument('--cold-driver',required=True);p.add_argument('--fault',choices=('pre-ready','post-ready'),required=True);a,rest=p.parse_known_args()
s=importlib.util.spec_from_file_location('root_actual_cold_fault',a.cold_driver);cold=importlib.util.module_from_spec(s);s.loader.exec_module(cold)
if a.fault=='pre-ready':
 original=cold.ColdHarness.ready
 def ready(self,process,path):
  original(self,process,path)
  if process is self.backend_process:
   self.report['root_injection']={'case':'pre-ready','trigger':'actual ready line read; before parent snapshot/receipt','known_record':str(self.backend_state/'@guest')}
   raise cold.census.Failed('ROOT pre-ready parent receipt-boundary fault')
 cold.ColdHarness.ready=ready
else:
 original_snapshot=cold.ColdHarness.snapshot;original_finish=cold.ColdHarness.finish_backend
 def snapshot(self):
  if getattr(self,'root_finishing',False) and not getattr(self,'root_fault_sent',False):
   self.root_fault_sent=True;self.report['root_injection']={'case':'post-ready','trigger':'first original finish snapshot after ready/class/live capacity verification'}
   raise cold.census.Failed('ROOT post-ready finish snapshot fault')
  return original_snapshot(self)
 def finish(self):
  self.root_finishing=True
  try:return original_finish(self)
  finally:self.root_finishing=False
 cold.ColdHarness.snapshot=snapshot;cold.ColdHarness.finish_backend=finish
sys.argv=[sys.argv[0],*rest];raise SystemExit(cold.main())
