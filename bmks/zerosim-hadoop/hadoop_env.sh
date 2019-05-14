

# Config env vars needed for hadoop

export JAVA_HOME=/usr/lib/jvm/jre-1.8.0-openjdk

export ZEROSIM_HADOOP_HOME=$HOME/zerosim-hadoop

export HADOOP_HOME=$ZEROSIM_HADOOP_HOME/hadoop-3.1.2
export HADOOP_CONF_DIR=$HADOOP_HOME/etc/hadoop
export HADOOP_MAPRED_HOME=$HADOOP_HOME
export HADOOP_COMMON_HOME=$HADOOP_HOME
export HADOOP_HDFS_HOME=$HADOOP_HOME
export YARN_HOME=$HADOOP_HOME

export SPARK_HOME=$ZEROSIM_HADOOP_HOME/spark-2.4.3-bin-hadoop2.7

export HIBENCH_HOME=$ZEROSIM_HADOOP_HOME/HiBench
