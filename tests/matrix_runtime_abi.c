#include <assert.h>
#include <math.h>
#include <stddef.h>
#include <stdio.h>
typedef struct SunMatrix SunMatrix;
SunMatrix *sun_matrix_create(size_t,size_t);void sun_matrix_free(SunMatrix*);
size_t sun_matrix_rows(const SunMatrix*);size_t sun_matrix_cols(const SunMatrix*);
int sun_matrix_set(SunMatrix*,size_t,size_t,double);
double sun_matrix_get(const SunMatrix*,size_t,size_t,int*);
SunMatrix *sun_matrix_add(const SunMatrix*,const SunMatrix*);
SunMatrix *sun_matrix_element_multiply(const SunMatrix*,const SunMatrix*);
SunMatrix *sun_matrix_multiply(const SunMatrix*,const SunMatrix*);
SunMatrix *sun_matrix_scale(const SunMatrix*,double);
SunMatrix *sun_matrix_divide_scalar(const SunMatrix*,double);
int main(void){
 SunMatrix*a=sun_matrix_create(2,3),*b=sun_matrix_create(2,3);assert(a&&b);
 assert(sun_matrix_rows(a)==2&&sun_matrix_cols(a)==3);
 for(size_t r=1;r<=2;r++)for(size_t c=1;c<=3;c++){assert(!sun_matrix_set(a,r,c,(r-1)*3+c));assert(!sun_matrix_set(b,r,c,1));}
 int st=-1;assert(fabs(sun_matrix_get(a,2,3,&st)-6)<1e-12&&st==0);
 (void)sun_matrix_get(a,0,1,&st);assert(st==2);
 SunMatrix*s=sun_matrix_add(a,b),*h=sun_matrix_element_multiply(a,b),*d=sun_matrix_scale(a,2),*q=sun_matrix_divide_scalar(a,2);
 assert(s&&h&&d&&q);assert(fabs(sun_matrix_get(s,2,3,&st)-7)<1e-12);
 assert(fabs(sun_matrix_get(h,2,3,&st)-6)<1e-12);assert(fabs(sun_matrix_get(d,2,3,&st)-12)<1e-12);
 assert(fabs(sun_matrix_get(q,2,3,&st)-3)<1e-12);
 SunMatrix*x=sun_matrix_create(3,2);assert(x);
 for(size_t r=1;r<=3;r++)for(size_t c=1;c<=2;c++)assert(!sun_matrix_set(x,r,c,1));
 SunMatrix*p=sun_matrix_multiply(a,x);assert(p&&sun_matrix_rows(p)==2&&sun_matrix_cols(p)==2);
 assert(fabs(sun_matrix_get(p,2,2,&st)-15)<1e-12);
 assert(!sun_matrix_create(0,2)&&!sun_matrix_divide_scalar(a,0));
 sun_matrix_free(p);sun_matrix_free(x);sun_matrix_free(q);sun_matrix_free(d);sun_matrix_free(h);sun_matrix_free(s);sun_matrix_free(b);sun_matrix_free(a);
 puts("matrix runtime ABI OK");return 0;
}