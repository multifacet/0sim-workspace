SRCS = identity_map \
	   check_mapping \
	   apriori_paging_reset_stats \
	   apriori_paging_set_process

LIBS = -lpthread
all: ${SRCS}

CC = gcc
CFLAGS = -O2 -Wall -g
PIE_CFLAGS = -fpie -pie

%:%.c
	${CC} ${CFLAGS} ${PIE_CFLAGS} -o $@ $< ${LIBS} 

clean:
	rm -f ${SRCS} *.o *.s 
