// goal 6 lever (1) sizing, indicative: openat2(O_PATH) of Docker's /proc targets resolved in the root
// (RESOLVE_IN_ROOT from "/") against beneath one opened /proc (RESOLVE_BENEATH from /proc's fd).
#define _GNU_SOURCE
#include <fcntl.h>
#include <linux/openat2.h>
#include <stdio.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>
static const char *T[] = {"acpi","asound","interrupts","kcore","keys","latency_stats","timer_list","sched_debug","scsi","bus","fs","irq","sys","sysrq-trigger"};
static double now(void){struct timespec t;clock_gettime(CLOCK_MONOTONIC,&t);return t.tv_sec*1e9+t.tv_nsec;}
static long o2(int d,const char*p,unsigned long long r){struct open_how h={.flags=O_PATH|O_CLOEXEC,.resolve=r};return syscall(SYS_openat2,d,p,&h,sizeof h);}
int main(void){int root=open("/",O_PATH|O_DIRECTORY|O_CLOEXEC); int n=sizeof T/sizeof*T, R=20000; char buf[64];
 for(int pass=0;pass<3;pass++){
  double a=now(); long ok=0;
  for(int r=0;r<R;r++) for(int i=0;i<n;i++){snprintf(buf,sizeof buf,"/proc/%s",T[i]); long f=o2(root,buf,RESOLVE_IN_ROOT); if(f>=0){ok++;close(f);}}
  double b=now();
  for(int r=0;r<R;r++){ long p=o2(root,"/proc",RESOLVE_IN_ROOT); for(int i=0;i<n;i++){long f=o2(p,T[i],RESOLVE_BENEATH); if(f>=0)close(f);} close(p);}
  double c=now();
  printf("pass %d: in root %.0f ns/target (%ld found), beneath /proc %.0f ns/target incl. its one open\n",pass,(b-a)/(R*n),ok/R,(c-b)/(R*n));}
 return 0;}
