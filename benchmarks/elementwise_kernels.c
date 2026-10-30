/* Build: clang -O3 -std=c11 -Wall -Wextra benchmarks/elementwise_kernels.c -lm -o elementwise-bench */
#define _POSIX_C_SOURCE 200809L
#include "../runtime/sun_runtime.c"
#include <time.h>

static double seconds(void) {
 struct timespec ts;
 if(clock_gettime(CLOCK_MONOTONIC,&ts)!=0)return 0.0;
 return (double)ts.tv_sec+(double)ts.tv_nsec/1e9;
}
static void fill(SunMatrix*m) {
 for(size_t i=0;i<m->rows*m->cols;i++) {
  m->data[i]=(double)((int)(i%101)-50)/17.0;
 }
}
static int close_enough(double a,double b) {
 return fabs(a-b)<=1e-12*fmax(1.0,fmax(fabs(a),fabs(b)));
}
typedef SunMatrix *(*binary_kernel)(const SunMatrix*,const SunMatrix*);
typedef SunMatrix *(*unary_kernel)(const SunMatrix*,double);
static double baseline_binary(const SunMatrix*a,const SunMatrix*b,int multiply,unsigned reps) {
 const size_t n=a->rows*a->cols;
 double *out=malloc(n*sizeof(double));if(!out)return -1.0;
 double start=seconds();
 for(unsigned r=0;r<reps;r++)for(size_t i=0;i<n;i++)
  out[i]=multiply?a->data[i]*b->data[i]:a->data[i]+b->data[i];
 double elapsed=(seconds()-start)/(double)reps;
 volatile double sink=out[n-1];(void)sink;free(out);return elapsed;
}
static double baseline_unary(const SunMatrix*a,double scalar,int divide,unsigned reps) {
 const size_t n=a->rows*a->cols;
 double *out=malloc(n*sizeof(double));if(!out)return -1.0;
 double start=seconds();
 for(unsigned r=0;r<reps;r++)for(size_t i=0;i<n;i++)
  out[i]=divide?a->data[i]/scalar:a->data[i]*scalar;
 double elapsed=(seconds()-start)/(double)reps;
 volatile double sink=out[n-1];(void)sink;free(out);return elapsed;
}
static int run_case(size_t rows,size_t cols,unsigned reps) {
 SunMatrix*a=sun_matrix_create(rows,cols),*b=sun_matrix_create(rows,cols);
 if(!a||!b)return 0;fill(a);fill(b);
 const size_t n=rows*cols;
 binary_kernel bk[]={sun_matrix_add,sun_matrix_element_multiply};
 const char*bn[]={"add","element_multiply"};
 for(int op=0;op<2;op++) {
  SunMatrix*out=bk[op](a,b);if(!out)return 0;
  for(size_t i=0;i<n;i++) {
   double expected=op?a->data[i]*b->data[i]:a->data[i]+b->data[i];
   if(!close_enough(out->data[i],expected)){fprintf(stderr,"binary mismatch n=%zu\\n",n);return 0;}
  }
  sun_matrix_free(out);
  double start=seconds();
  for(unsigned r=0;r<reps;r++){out=bk[op](a,b);if(!out)return 0;sun_matrix_free(out);}
  double vector_time=(seconds()-start)/(double)reps;
  double scalar_time=baseline_binary(a,b,op,reps);if(scalar_time<0)return 0;
  printf("op=%s shape=%zux%zu scalar_seconds=%.8f runtime_seconds=%.8f ratio=%.3f\\n",
   bn[op],rows,cols,scalar_time,vector_time,scalar_time/vector_time);
 }
 unary_kernel uk[]={sun_matrix_scale,sun_matrix_divide_scalar};
 const char*un[]={"scale","divide_scalar"};
 for(int op=0;op<2;op++) {
  const double scalar=3.25;
  SunMatrix*out=uk[op](a,scalar);if(!out)return 0;
  for(size_t i=0;i<n;i++) {
   double expected=op?a->data[i]/scalar:a->data[i]*scalar;
   if(!close_enough(out->data[i],expected)){fprintf(stderr,"unary mismatch n=%zu\\n",n);return 0;}
  }
  sun_matrix_free(out);
  double start=seconds();
  for(unsigned r=0;r<reps;r++){out=uk[op](a,scalar);if(!out)return 0;sun_matrix_free(out);}
  double vector_time=(seconds()-start)/(double)reps;
  double scalar_time=baseline_unary(a,scalar,op,reps);if(scalar_time<0)return 0;
  printf("op=%s shape=%zux%zu scalar_seconds=%.8f runtime_seconds=%.8f ratio=%.3f\\n",
   un[op],rows,cols,scalar_time,vector_time,scalar_time/vector_time);
 }
 sun_matrix_free(a);sun_matrix_free(b);return 1;
}
int main(int argc,char**argv) {
 unsigned reps=10;if(argc>1)reps=(unsigned)strtoul(argv[1],NULL,10);if(!reps)reps=1;
 const size_t shapes[][2]={{1,1},{1,3},{31,33},{32,32},{33,31},{128,256},{257,255},{512,512}};
 for(size_t i=0;i<sizeof(shapes)/sizeof(shapes[0]);i++)
  if(!run_case(shapes[i][0],shapes[i][1],reps)){sun_runtime_cleanup();return EXIT_FAILURE;}
 sun_runtime_cleanup();return EXIT_SUCCESS;
}
