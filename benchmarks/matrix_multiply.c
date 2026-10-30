/* Standalone benchmark: cc -O3 -std=c11 -Wall -Wextra benchmarks/matrix_multiply.c -lm -o matrix-bench */
#define _POSIX_C_SOURCE 200809L
#include "../runtime/sun_runtime.c"
#include <time.h>

static double now_seconds(void) {
 struct timespec ts;
 if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return 0.0;
 return (double)ts.tv_sec + (double)ts.tv_nsec / 1e9;
}
static void fill_matrix(SunMatrix *m, size_t rows, size_t cols, unsigned seed) {
 for (size_t i=0;i<rows*cols;i++) {
  seed = seed * 1664525u + 1013904223u;
  m->data[i] = ((double)(seed >> 8) / 16777216.0) - 0.5;
 }
}
static void reference_multiply(const SunMatrix *a, const SunMatrix *b, SunMatrix *out) {
 for(size_t i=0;i<a->rows;i++)
  for(size_t j=0;j<b->cols;j++)
   for(size_t k=0;k<a->cols;k++)
    out->data[i*out->cols+j] += a->data[i*a->cols+k]*b->data[k*b->cols+j];
}
static double checksum(const SunMatrix *m) {
 double sum=0.0;
 for(size_t i=0;i<m->rows*m->cols;i++) sum += m->data[i];
 return sum;
}
static int run_case(size_t n, unsigned repetitions) {
 SunMatrix *a=sun_matrix_create(n,n), *b=sun_matrix_create(n,n);
 SunMatrix *reference=sun_matrix_create(n,n);
 if(!a||!b||!reference) return 0;
 fill_matrix(a,n,n,1234u); fill_matrix(b,n,n,5678u);
 double start=now_seconds();
 reference_multiply(a,b,reference);
 double reference_seconds=now_seconds()-start;
 SunMatrix *optimized=sun_matrix_multiply(a,b);
 if(!optimized) return 0;
 double max_error=0.0;
 for(size_t i=0;i<n*n;i++) {
  double error=fabs(reference->data[i]-optimized->data[i]);
  if(error>max_error) max_error=error;
 }
 if(max_error>1e-9*(double)n) {
  fprintf(stderr,"correctness failure n=%zu max_error=%.3e\\n",n,max_error);
  return 0;
 }
 for(unsigned r=0;r<repetitions;r++) {
  sun_matrix_free(optimized);
  start=now_seconds();
  optimized=sun_matrix_multiply(a,b);
  double elapsed=now_seconds()-start;
  if(!optimized) return 0;
  printf("kernel=blocked n=%zu repetition=%u seconds=%.6f gflops=%.3f\\n",
   n,r+1,elapsed,(2.0*(double)n*(double)n*(double)n)/(elapsed*1e9));
 }
 start=now_seconds();
 for(unsigned r=0;r<repetitions;r++) {
  SunMatrix *tmp=sun_matrix_create(n,n);
  if(!tmp) return 0;
  reference_multiply(a,b,tmp);
  sun_matrix_free(tmp);
 }
 double baseline_seconds=(now_seconds()-start)/(double)repetitions;
 printf("summary n=%zu baseline_naive_seconds=%.6f blocked_last_checksum=%.12g max_abs_error=%.3e\\n",
  n,reference_seconds,checksum(optimized),max_error);
 printf("comparison n=%zu baseline_avg_seconds=%.6f blocked_avg_seconds=see_kernel_lines\\n",
  n,baseline_seconds);
 sun_matrix_free(a);sun_matrix_free(b);sun_matrix_free(reference);sun_matrix_free(optimized);
 return 1;
}
int main(int argc,char **argv) {
 unsigned repetitions=3;
 if(argc>1) repetitions=(unsigned)strtoul(argv[1],NULL,10);
 if(repetitions==0) repetitions=1;
 const size_t sizes[]={64,128,256,512};
 for(size_t i=0;i<sizeof(sizes)/sizeof(sizes[0]);i++)
  if(!run_case(sizes[i],repetitions)) { sun_runtime_cleanup(); return EXIT_FAILURE; }
 sun_runtime_cleanup();
 return EXIT_SUCCESS;
}
