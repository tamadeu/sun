#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
typedef struct SunMatrix { size_t rows, cols; double *data; } SunMatrix;
enum { SUN_MATRIX_OK=0, SUN_MATRIX_ERR_NULL=1, SUN_MATRIX_ERR_BOUNDS=2 };
double sun_rt_sqrt(double x){return sqrt(x);} double sun_rt_sin(double x){return sin(x);}
double sun_rt_cos(double x){return cos(x);} double sun_rt_exp(double x){return exp(x);}
double sun_rt_log(double x){return log(x);} void sun_rt_print_f64(double x){printf("%g\n",x);}
SunMatrix *sun_matrix_create(size_t r,size_t c){
 if(!r||!c||r>SIZE_MAX/c||r*c>SIZE_MAX/sizeof(double))return NULL;
 SunMatrix*m=malloc(sizeof(*m));if(!m)return NULL;m->data=calloc(r*c,sizeof(double));
 if(!m->data){free(m);return NULL;}m->rows=r;m->cols=c;return m;
}
void sun_matrix_free(SunMatrix*m){if(m){free(m->data);free(m);}}
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
double sun_matrix_get_indexed(const SunMatrix*m,double row,double col){
 if(!m){fprintf(stderr,"Sun runtime error: indexing a null matrix\\n");exit(EXIT_FAILURE);}
 if(!isfinite(row)||floor(row)!=row||row<1.0||row>(double)m->rows){
  fprintf(stderr,"Sun runtime error: row index must be an integer in [1, %zu], got %g\\n",m->rows,row);
  exit(EXIT_FAILURE);
 }
 if(!isfinite(col)||floor(col)!=col||col<1.0||col>(double)m->cols){
  fprintf(stderr,"Sun runtime error: column index must be an integer in [1, %zu], got %g\\n",m->cols,col);
  exit(EXIT_FAILURE);
 }
 return m->data[((size_t)row-1)*m->cols+((size_t)col-1)];
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
SunMatrix *sun_matrix_multiply(const SunMatrix*a,const SunMatrix*b){
 if(!a||!b||a->cols!=b->rows)return NULL;SunMatrix*o=sun_matrix_create(a->rows,b->cols);if(!o)return NULL;
 for(size_t i=0;i<a->rows;i++) {
  for(size_t j=0;j<b->cols;j++) {
   for(size_t k=0;k<a->cols;k++) {
    o->data[i*o->cols+j]+=a->data[i*a->cols+k]*b->data[k*b->cols+j];
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
