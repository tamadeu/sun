#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
typedef struct SunMatrix { size_t rows, cols; double *data; struct SunMatrix *next_owned; } SunMatrix;
static SunMatrix *sun_owned_matrices = NULL;
void sun_runtime_cleanup(void);

static void sun_runtime_fail(const char *message) {
 sun_runtime_cleanup();
 fprintf(stderr, "Sun runtime error: %s\n", message);
 exit(EXIT_FAILURE);
}
void sun_runtime_cleanup(void) {
 while (sun_owned_matrices) {
  SunMatrix *matrix = sun_owned_matrices;
  sun_owned_matrices = matrix->next_owned;
  free(matrix->data);
  free(matrix);
 }
}
void sun_runtime_check_matrix(SunMatrix *matrix) {
 if (!matrix) sun_runtime_fail("matrix allocation or operation failed");
}
enum { SUN_MATRIX_OK=0, SUN_MATRIX_ERR_NULL=1, SUN_MATRIX_ERR_BOUNDS=2 };
double sun_rt_sqrt(double x){return sqrt(x);} double sun_rt_sin(double x){return sin(x);}
double sun_rt_cos(double x){return cos(x);} double sun_rt_exp(double x){return exp(x);}
double sun_rt_log(double x){return log(x);} void sun_rt_print_f64(double x){printf("%g\n",x);}
SunMatrix *sun_matrix_create(size_t r,size_t c){
 if(!r||!c||r>SIZE_MAX/c||r*c>SIZE_MAX/sizeof(double))return NULL;
 SunMatrix*m=malloc(sizeof(*m));if(!m)return NULL;m->data=calloc(r*c,sizeof(double));
 if(!m->data){free(m);return NULL;}m->rows=r;m->cols=c;m->next_owned=sun_owned_matrices;sun_owned_matrices=m;return m;
}
void sun_matrix_free(SunMatrix*m){if(m){SunMatrix **link=&sun_owned_matrices;while(*link&&*link!=m)link=&(*link)->next_owned;if(*link==m)*link=m->next_owned;free(m->data);free(m);}}
size_t sun_matrix_rows(const SunMatrix*m){return m?m->rows:0;}
size_t sun_matrix_cols(const SunMatrix*m){return m?m->cols:0;}
int sun_matrix_set(SunMatrix*m,size_t r,size_t c,double v){
 if(!m)return SUN_MATRIX_ERR_NULL;if(!r||!c||r>m->rows||c>m->cols)return SUN_MATRIX_ERR_BOUNDS;
 m->data[(r-1)*m->cols+c-1]=v;return SUN_MATRIX_OK;
}
double sun_matrix_get(const SunMatrix*m,size_t r,size_t c,int*status){
 int s=SUN_MATRIX_OK;double v=0;if(!m)s=SUN_MATRIX_ERR_NULL;
 else if(!r||!c||r>m->rows||c>m->cols)s=SUN_MATRIX_ERR_BOUNDS;
 else v=m->data[(r-1)*m->cols+c-1];if(status)*status=s;return v;
}
void sun_matrix_set_indexed(SunMatrix *m, double row, double col, double value) {
 if (!m) sun_runtime_fail("assigning into a null matrix");
 if (!isfinite(row) || floor(row) != row || row < 1.0 || row > (double)m->rows)
  sun_runtime_fail("row index is not a finite in-bounds positive integer");
 if (!isfinite(col) || floor(col) != col || col < 1.0 || col > (double)m->cols)
  sun_runtime_fail("column index is not a finite in-bounds positive integer");
 m->data[((size_t)row - 1) * m->cols + ((size_t)col - 1)] = value;
}
double sun_matrix_get_indexed(const SunMatrix *m, double row, double col) {
 if (!m) sun_runtime_fail("indexing a null matrix");
 if (!isfinite(row) || floor(row) != row || row < 1.0 || row > (double)m->rows)
  sun_runtime_fail("row index is not a finite in-bounds positive integer");
 if (!isfinite(col) || floor(col) != col || col < 1.0 || col > (double)m->cols)
  sun_runtime_fail("column index is not a finite in-bounds positive integer");
 return m->data[((size_t)row - 1) * m->cols + ((size_t)col - 1)];
}
SunMatrix *sun_matrix_add(const SunMatrix*a,const SunMatrix*b){
 if(!a||!b||a->rows!=b->rows||a->cols!=b->cols)return NULL;
 SunMatrix*o=sun_matrix_create(a->rows,a->cols);if(!o)return NULL;
 for(size_t i=0;i<a->rows*a->cols;i++)o->data[i]=a->data[i]+b->data[i];return o;
}
SunMatrix *sun_matrix_element_multiply(const SunMatrix*a,const SunMatrix*b){
 if(!a||!b||a->rows!=b->rows||a->cols!=b->cols)return NULL;
 SunMatrix*o=sun_matrix_create(a->rows,a->cols);if(!o)return NULL;
 for(size_t i=0;i<a->rows*a->cols;i++)o->data[i]=a->data[i]*b->data[i];return o;
}

/* Cache-blocked row-major matrix multiplication. The inner j loop walks
 * contiguous rows of B and C; k remains ascending for each output element. */
#define SUN_MATRIX_MATMUL_BLOCK 32
SunMatrix *sun_matrix_multiply(const SunMatrix*a,const SunMatrix*b){
 if(!a||!b||a->cols!=b->rows)return NULL;
 SunMatrix*o=sun_matrix_create(a->rows,b->cols);if(!o)return NULL;
 const size_t block=SUN_MATRIX_MATMUL_BLOCK;
 for(size_t ii=0;ii<a->rows;ii+=block){
  const size_t i_end=ii+((a->rows-ii)<block?(a->rows-ii):block);
  for(size_t kk=0;kk<a->cols;kk+=block){
   const size_t k_end=kk+((a->cols-kk)<block?(a->cols-kk):block);
   for(size_t jj=0;jj<b->cols;jj+=block){
    const size_t j_end=jj+((b->cols-jj)<block?(b->cols-jj):block);
    for(size_t i=ii;i<i_end;i++){
     double *restrict c_row=o->data+i*o->cols;
     const double *a_row=a->data+i*a->cols;
     for(size_t k=kk;k<k_end;k++){
      const double aik=a_row[k];
      const double *b_row=b->data+k*b->cols;
      for(size_t j=jj;j<j_end;j++)c_row[j]+=aik*b_row[j];
     }
    }
   }
  }
 }
 return o;
}
SunMatrix *sun_matrix_scale(const SunMatrix*a,double x){
 if(!a)return NULL;SunMatrix*o=sun_matrix_create(a->rows,a->cols);if(!o)return NULL;
 for(size_t i=0;i<a->rows*a->cols;i++)o->data[i]=a->data[i]*x;return o;
}
SunMatrix *sun_matrix_divide_scalar(const SunMatrix*a,double x){
 if(!a||x==0.0)return NULL;SunMatrix*o=sun_matrix_create(a->rows,a->cols);if(!o)return NULL;
 for(size_t i=0;i<a->rows*a->cols;i++)o->data[i]=a->data[i]/x;return o;
}
void sun_matrix_print(const SunMatrix*m){
 if(!m){puts("<null matrix>");return;}for(size_t r=0;r<m->rows;r++){
 for(size_t c=0;c<m->cols;c++)printf("%g%s",m->data[r*m->cols+c],c+1==m->cols?"":" ");
 putchar('\n');}
}
